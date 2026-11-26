"""What the Odoo extension knows, without Florm: test tags, commands, models and records."""

import os
import re
import shutil
import subprocess

SKIP_DIRS = {".git", "node_modules", "__pycache__", ".venv", "venv", "static", "i18n", "filestores", "pg_data"}


def is_test_file(path):
    """A test file of an Odoo module: tests/.../test_*.py."""
    name = os.path.basename(path)
    return name.startswith("test") and name.endswith(".py") and "tests" in path.replace("\\", "/").split("/")


def module_dir(path):
    """The folder of the Odoo module of path (the one holding __manifest__.py), or None."""
    d = os.path.dirname(os.path.abspath(path))
    while True:
        if os.path.isfile(os.path.join(d, "__manifest__.py")):
            return d
        parent = os.path.dirname(d)
        if parent == d:
            return None
        d = parent


def module_of(path):
    d = module_dir(path)
    return os.path.basename(d) if d else None


def tests_in(text):
    """The test classes and methods of a Python text: (line from 0, ':Class' or ':Class.test_x')."""
    out = []
    cls = None
    for i, line in enumerate(text.splitlines()):
        t = line.lstrip()
        indent = len(line) - len(t)
        if not t or t.startswith("#"):
            continue
        if cls and indent <= cls[1]:
            cls = None
        if t.startswith("class "):
            rest = t[6:]
            name = re.match(r"\w*", rest).group(0)
            bases = rest.split("(", 1)[1] if "(" in rest else ""
            if name and (name.startswith("Test") or name.endswith(("Test", "Tests", "Case")) or "Case" in bases):
                out.append((i, ":" + name))
                cls = (name, indent)
            continue
        if cls:
            m = re.match(r"(?:async\s+)?def\s+(test\w*)", t)
            if m:
                out.append((i, ":%s.%s" % (cls[0], m.group(1))))
    return out


def tags_at(path, text, line):
    """The --test-tags of the code at line (from 0): the test method or class around it, with its
    module (/sale:TestX.test_y); else the module of the file (/sale)."""
    module = module_of(path)
    if is_test_file(path):
        lines = text.splitlines()

        def indent(s):
            return len(s) - len(s.lstrip())

        def contains(start):
            i0 = indent(lines[start])
            return all(k >= len(lines) or not lines[k].strip() or indent(lines[k]) > i0 for k in range(start + 1, line + 1))

        for at, tag in reversed(tests_in(text)):
            if at <= line and contains(at):
                return ("/" + module if module else "") + tag
    return "/" + module if module else None


def is_command(cmd):
    """An Odoo test command: it has --test-tags."""
    return any(w == "--test-tags" or w.startswith("--test-tags=") for w in cmd.split())


def _replace_option(cmd, option, value):
    """cmd with the value of option (--test-tags x or --test-tags=x) changed; None if absent."""
    words = cmd.split(" ")
    out = []
    found = False
    i = 0
    while i < len(words):
        w = words[i]
        if w == option:
            found = True
            out.append(w)
            i += 1
            while i < len(words) and words[i] == "":
                i += 1
            out.append(value)
        elif w.startswith(option + "="):
            found = True
            out.append(option + "=" + value)
        else:
            out.append(w)
        i += 1
    return " ".join(out) if found else None


def command(tags, last=None, odoo_bin=None):
    """The command running the tests tags: the last one with its --test-tags changed (it keeps
    the database, -u, the container...), else odoo-bin with them."""
    if last and is_command(last):
        return _replace_option(last, "--test-tags", tags)
    return "%s --stop-after-init --test-enable --test-tags %s" % (odoo_bin or "odoo", tags)


def update_command(module, last=None, odoo_bin=None):
    """The command updating module: the last test command without its tests, with -u module."""
    if last and is_command(last):
        words = []
        skip = False
        for w in last.split():
            if skip:
                skip = False
                continue
            if w in ("--test-tags", "-u", "--update"):
                skip = True
                continue
            if w.startswith(("--test-tags=", "--update=")) or w == "--test-enable":
                continue
            words.append(w)
        cmd = " ".join(words)
        if "--stop-after-init" not in cmd.split():
            cmd += " --stop-after-init"
        return cmd + " -u " + module
    return "%s --stop-after-init -u %s" % (odoo_bin or "odoo", module)


def find_odoo_bin(root):
    """odoo-bin in root or two folders under it: how to run it from root (./odoo/odoo-bin)."""
    if not root:
        return None
    level = [root]
    for _ in range(3):
        for d in level:
            if os.path.isfile(os.path.join(d, "odoo-bin")):
                rel = os.path.relpath(os.path.join(d, "odoo-bin"), root).replace("\\", "/")
                # Windows does not run a script by its #! line: through Python
                return ("python " if os.name == "nt" else "") + "./" + rel
        nxt = []
        for d in level:
            try:
                nxt.extend(sorted(os.path.join(d, e) for e in os.listdir(d) if not e.startswith(".") and e not in SKIP_DIRS and os.path.isdir(os.path.join(d, e))))
            except OSError:
                pass
        level = nxt
    return None


def word_at(line, col):
    """What is under the cursor (col from 0): a quoted string ('sale.order'), else a dotted name."""
    for m in re.finditer(r"""(['"])([\w.]+)\1""", line):
        if m.start() <= col <= m.end():
            return m.group(2)
    for m in re.finditer(r"[\w.]+", line):
        if m.start() <= col <= m.end():
            return m.group(0).strip(".")
    return ""


def _grep(root, pattern, globs):
    """(path, line from 1, text) of the lines matching pattern (a regex) under root."""
    out = []
    rg = shutil.which("rg")
    if rg:
        cmd = [rg, "-n", "--no-heading", "--color=never", "-e", pattern]
        for g in globs:
            cmd += ["-g", g]
        for d in SKIP_DIRS:
            cmd += ["-g", "!" + d]
        cmd.append(root)
        try:
            text = subprocess.run(cmd, capture_output=True, text=True, timeout=60).stdout
        except (OSError, subprocess.TimeoutExpired):
            text = ""
        for l in text.splitlines():
            path, _, rest = l.partition(":")
            num, _, body = rest.partition(":")
            if num.isdigit():
                out.append((path, int(num), body))
        return out
    rx = re.compile(pattern)
    exts = tuple(g.lstrip("*") for g in globs)
    for d, dirs, files in os.walk(root):
        dirs[:] = [x for x in dirs if x not in SKIP_DIRS and not x.startswith(".")]
        for f in files:
            if f.endswith(exts):
                p = os.path.join(d, f)
                try:
                    with open(p, encoding="utf-8", errors="replace") as fh:
                        for i, l in enumerate(fh, 1):
                            if rx.search(l):
                                out.append((p, i, l.rstrip("\n")))
                except OSError:
                    pass
    return out


def find_model(root, model):
    """Where model is defined (_name = 'model'), then where it is extended (_inherit)."""
    m = re.escape(model)
    defs = _grep(root, r"""^\s*_name\s*=\s*['"]%s['"]""" % m, ["*.py"])
    ext = [h for h in _grep(root, r"""^\s*_inherit\s*=\s*.*['"]%s['"]""" % m, ["*.py"]) if not any(h[0] == d[0] for d in defs)]
    # the module named like the model first (sale for sale.order), then by path
    home = "/%s/" % model.split(".")[0]

    def order(h):
        return (home not in h[0].replace("\\", "/"), h[0], h[1])

    return [(p, n, "_name", t.strip()) for p, n, t in sorted(defs, key=order)] + [(p, n, "_inherit", t.strip()) for p, n, t in sorted(ext, key=order)]


def find_xmlid(root, xmlid, current_module=None):
    """Where the record module.xml_id is (id="xml_id" in the module's XML or CSV files)."""
    module, _, name = xmlid.rpartition(".")
    module = module or current_module
    hits = _grep(root, r"""\bid=["']%s["']""" % re.escape(name), ["*.xml"])
    hits += [h for h in _grep(root, r"^%s," % re.escape(name), ["*.csv"])]
    if module:
        mine = [h for h in hits if ("/%s/" % module) in h[0].replace("\\", "/")]
        hits = mine or hits
    return [(p, n, "record", t.strip()) for p, n, t in hits]


def records_in(path, text):
    """The models and records of a file: (line from 1, label)."""
    out = []
    if path.endswith(".xml"):
        for i, l in enumerate(text.splitlines(), 1):
            m = re.search(r"<(record|template|menuitem|function)\b[^>]*", l)
            if not m:
                continue
            tag = m.group(1)
            rid = re.search(r"""\bid=["']([^"']+)""", m.group(0))
            model = re.search(r"""\bmodel=["']([^"']+)""", m.group(0))
            label = rid.group(1) if rid else "?"
            if tag != "record":
                label = "%s  (%s)" % (label, tag)
            elif model:
                label = "%s  (%s)" % (label, model.group(1))
            out.append((i, label))
    elif path.endswith(".py"):
        cls = None
        for i, l in enumerate(text.splitlines(), 1):
            m = re.match(r"class\s+(\w+)\s*\(", l)
            if m:
                cls = (i, m.group(1))
                continue
            m = re.match(r"""\s+_(name|inherit)\s*=\s*(.+)""", l)
            if m and cls:
                names = re.findall(r"""['"]([\w.]+)['"]""", m.group(2))
                if names:
                    out.append((cls[0], "%s  %s %s" % (cls[1], "_" + m.group(1), ", ".join(names))))
                    cls = None
    return out
