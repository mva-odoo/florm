#!/usr/bin/env python3
"""Small benchmark of editors (Florm, Sublime Text, VS Code) on a folder: startup, RAM, CPU, disk.

Each editor starts clean and apart from the ones already open (Florm with an empty config,
so without Claude; Sublime in --safe-mode; VS Code with a temporary profile). Its processes
are found by a variable of their environment (inherited by all of them), measured every
half second, then the editor is closed.

    python3 scripts/bench.py ~/src/odoo/master [--editors florm,sublime,code] [--settle 120] [--idle 30]

Measures (Linux, /proc):
- window: seconds until its window shows (Hyprland: hyprctl; else not measured)
- settled: seconds until the CPU stays under 3 % for 5 s (folder read, index made)
- RAM: PSS + swap of all its processes (PSS: the shared pages split between processes),
  at the peak and at the end; the process count
- CPU: seconds of CPU used to settle, then the average % at rest over --idle seconds
- disk: bytes read and written by its processes (their own I/O, not the page cache)
"""
import argparse
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time

HZ = os.sysconf("SC_CLK_TCK")
TAG = "FLORM_BENCH_TAG"


def tagged(tag, mark=None):
    """The processes of an editor: those whose environment carries `tag` or whose command line
    holds `mark` (Electron rewrites the memory of its environment), and all their descendants."""
    parent, seeds = {}, set()
    for p in os.listdir("/proc"):
        if not p.isdigit():
            continue
        pid = int(p)
        try:
            with open(f"/proc/{p}/stat") as f:
                parent[pid] = int(f.read().rsplit(")", 1)[1].split()[1])
            with open(f"/proc/{p}/environ", "rb") as f:
                if f"{TAG}={tag}".encode() in f.read().split(b"\0"):
                    seeds.add(pid)
                    continue
            if mark:
                with open(f"/proc/{p}/cmdline", "rb") as f:
                    if mark.encode() in f.read():
                        seeds.add(pid)
        except (OSError, IndexError, ValueError):
            pass
    seeds.discard(os.getpid())
    found = set(seeds)
    grew = True
    while grew:
        grew = False
        for pid, pp in parent.items():
            if pp in found and pid not in found:
                found.add(pid)
                grew = True
    return sorted(found)


def read_kv(path, keys):
    vals = dict.fromkeys(keys, 0)
    try:
        with open(path) as f:
            for line in f:
                k, _, v = line.partition(":")
                if k in vals:
                    vals[k] = int(v.split()[0])
    except OSError:
        pass
    return vals


def sample(pids):
    """(memory in kB: PSS + swap, CPU ticks, bytes read, bytes written) of `pids`."""
    mem = ticks = rd = wr = 0
    for p in pids:
        m = read_kv(f"/proc/{p}/smaps_rollup", ["Pss", "Swap"])
        mem += m["Pss"] + m["Swap"]
        try:
            with open(f"/proc/{p}/stat") as f:
                fields = f.read().rsplit(")", 1)[1].split()
            ticks += int(fields[11]) + int(fields[12])
        except (OSError, IndexError):
            pass
        io = read_kv(f"/proc/{p}/io", ["read_bytes", "write_bytes"])
        rd += io["read_bytes"]
        wr += io["write_bytes"]
    return mem, ticks, rd, wr


def has_window(pids):
    if not shutil.which("hyprctl"):
        return None
    try:
        clients = json.loads(subprocess.run(["hyprctl", "clients", "-j"], capture_output=True, text=True, timeout=2).stdout)
    except (OSError, ValueError, subprocess.TimeoutExpired):
        return None
    return any(c.get("pid") in pids for c in clients)


def launch(editor, folder, tmp):
    env = dict(os.environ)
    if editor == "florm":
        env["FLORM_CONFIG"] = os.path.join(tmp, "florm-config")
        cmd = ["florm", folder]
    elif editor == "sublime":
        cmd = ["subl", "--safe-mode", "--new-window", folder]
    elif editor == "code":
        cmd = ["code", "--new-window", "--user-data-dir", os.path.join(tmp, "code-data"), "--extensions-dir", os.path.join(tmp, "code-ext"),
               "--disable-workspace-trust", "--skip-welcome", "--skip-release-notes", folder]
    else:
        sys.exit(f"unknown editor: {editor}")
    return env, cmd


def bench(editor, folder, settle_max, idle_s):
    tag = f"{editor}-{os.getpid()}-{time.time_ns()}"
    tmp = tempfile.mkdtemp(prefix=f"bench-{editor}-")
    env, cmd = launch(editor, folder, tmp)
    env[TAG] = tag
    t0 = time.monotonic()
    subprocess.Popen(cmd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    window_at = settled_at = None
    peak_mem = peak_procs = 0
    quiet = 0
    last_ticks = None
    # ticks of the processes that exited are lost: the CPU is the sum of the steps seen
    cpu_ticks = 0
    prev = {}
    pids = []
    while True:
        time.sleep(0.5)
        now = time.monotonic() - t0
        pids = tagged(tag, tmp)
        mem, _, _, _ = sample(pids)
        step = 0
        cur = {}
        for p in pids:
            _, t, _, _ = sample([p])
            cur[p] = t
            step += t - prev.get(p, 0)
        prev = cur
        cpu_ticks += step
        peak_mem = max(peak_mem, mem)
        peak_procs = max(peak_procs, len(pids))
        if window_at is None and pids and has_window(set(pids)):
            window_at = now
        pct = step / HZ / 0.5 * 100
        quiet = quiet + 1 if pct < 3 and pids else 0
        if settled_at is None and quiet >= 10 and now > 8:
            settled_at = now
            settle_ticks = cpu_ticks
            break
        if now > settle_max:
            settle_ticks = cpu_ticks
            break
    # at rest
    idle_ticks = 0
    t_idle = time.monotonic()
    while time.monotonic() - t_idle < idle_s:
        time.sleep(0.5)
        pids = tagged(tag, tmp)
        for p in pids:
            _, t, _, _ = sample([p])
            idle_ticks += t - prev.get(p, 0)
            prev[p] = t
        peak_mem = max(peak_mem, sample(pids)[0])
    pids = tagged(tag, tmp)
    end_mem, _, rd, wr = sample(pids)
    procs = len(pids)
    for p in pids:
        try:
            os.kill(p, signal.SIGTERM)
        except OSError:
            pass
    time.sleep(2)
    for p in tagged(tag, tmp):
        try:
            os.kill(p, signal.SIGKILL)
        except OSError:
            pass
    shutil.rmtree(tmp, ignore_errors=True)
    return {
        "editor": editor,
        "window_s": window_at,
        "settled_s": settled_at,
        "settle_cpu_s": settle_ticks / HZ,
        "idle_cpu_pct": idle_ticks / HZ / idle_s * 100,
        "peak_mem_mb": peak_mem / 1024,
        "end_mem_mb": end_mem / 1024,
        "procs": max(procs, 0),
        "peak_procs": peak_procs,
        "read_mb": rd / 1e6,
        "write_mb": wr / 1e6,
    }


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("folder")
    ap.add_argument("--editors", default="florm,sublime,code")
    ap.add_argument("--settle", type=float, default=120, help="seconds at most to wait for it to settle")
    ap.add_argument("--idle", type=float, default=30, help="seconds measured at rest")
    a = ap.parse_args()
    folder = os.path.abspath(os.path.expanduser(a.folder))
    rows = []
    for e in a.editors.split(","):
        print(f"… {e} on {folder}", file=sys.stderr, flush=True)
        rows.append(bench(e, folder, a.settle, a.idle))
        time.sleep(3)
    fmt = lambda v, f: "—" if v is None else f.format(v)
    print(f"\nFolder: {folder}\n")
    print("| | window | settled | CPU to settle | CPU at rest | RAM peak | RAM end | processes | disk read | disk written |")
    print("|---|---|---|---|---|---|---|---|---|---|")
    for r in rows:
        settled = fmt(r["settled_s"], "{:.1f} s") if r["settled_s"] is not None else f"> {a.settle:.0f} s"
        print(f"| {r['editor']} | {fmt(r['window_s'], '{:.1f} s')} | {settled} | {r['settle_cpu_s']:.1f} s | {r['idle_cpu_pct']:.1f} % "
              f"| {r['peak_mem_mb']:.0f} MB | {r['end_mem_mb']:.0f} MB | {r['procs']} | {r['read_mb']:.0f} MB | {r['write_mb']:.0f} MB |")
    print("\n" + json.dumps(rows))


if __name__ == "__main__":
    main()
