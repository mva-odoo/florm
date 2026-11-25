"""The library of Florm's extensions (written by Florm next to them, on their PYTHONPATH).

    import florm

    @florm.command("upper")              # an id of [[command]] in extension.toml
    def upper(ctx):
        florm.replace_selection(ctx.selection.upper())

    @florm.on("save")                    # open, save, close, focus (events = [...])
    def saved(ctx):
        florm.status(f"{ctx.file} saved")

    florm.main()

ctx: file, root (the project), language, selection (its text), line and col (from 1).
print() goes to Florm's Output panel. An exception is shown in the status bar, its trace in Output.
"""

import json
import queue
import sys
import threading
import traceback

__all__ = [
    "command", "on", "main", "status", "message", "output", "text", "replace_selection",
    "insert", "set_text", "open", "pick", "input", "run", "terminal", "Context",
]

_commands = {}
_events = {}
_out = sys.stdout.buffer
_in = sys.stdin.buffer
# print() must not break the messages on stdout: it goes to stderr, read by Florm into Output
sys.stdout = sys.stderr
_lock = threading.Lock()
_next = [0]
_waiting = {}
_incoming = queue.Queue()
info = {}


class FlormError(Exception):
    pass


class Context:
    """Where Florm is when a command runs or an event comes."""

    def __init__(self, d):
        d = d or {}
        self.file = d.get("file")
        self.root = d.get("root")
        self.language = d.get("language", "")
        self.selection = d.get("selection", "")
        self.line = d.get("line", 0)
        self.col = d.get("col", 0)
        self.raw = d

    def __repr__(self):
        return f"Context(file={self.file!r}, line={self.line}, selection={self.selection[:20]!r})"


def _send(msg):
    body = json.dumps(msg).encode("utf-8")
    with _lock:
        _out.write(b"Content-Length: %d\r\n\r\n" % len(body))
        _out.write(body)
        _out.flush()


def _read():
    length = None
    while True:
        line = _in.readline()
        if not line:
            return None
        line = line.strip()
        if not line:
            break
        k, _, v = line.decode("ascii", "replace").partition(":")
        if k.lower() == "content-length":
            length = int(v.strip())
    if length is None:
        return None
    return json.loads(_in.read(length).decode("utf-8"))


def _reader():
    while True:
        msg = _read()
        if msg is None:
            _incoming.put(None)
            return
        if "method" not in msg and "id" in msg:
            w = _waiting.pop(msg["id"], None)
            if w is not None:
                w[1] = msg
                w[0].set()
            continue
        _incoming.put(msg)


def _request(method, params=None):
    with _lock:
        _next[0] += 1
        rid = f"x{_next[0]}"
    w = [threading.Event(), None]
    _waiting[rid] = w
    _send({"jsonrpc": "2.0", "id": rid, "method": method, "params": params or {}})
    w[0].wait()
    msg = w[1]
    if "error" in msg:
        raise FlormError(msg["error"].get("message", str(msg["error"])))
    return msg.get("result")


def _notify(method, params=None):
    _send({"jsonrpc": "2.0", "method": method, "params": params or {}})


# ------------------------------------------------------------------ what an extension declares

def command(id):
    """The function of a command of extension.toml ([[command]] id = ...)."""
    def wrap(f):
        _commands[id] = f
        return f
    return wrap


def on(event):
    """A function called on an event of extension.toml (events = ["save", ...])."""
    def wrap(f):
        _events.setdefault(event, []).append(f)
        return f
    return wrap


# ------------------------------------------------------------------ what an extension can do

def status(text):
    """A message in the status bar."""
    _notify("status", {"text": str(text)})


def message(text, level="info"):
    """A message in the status bar: info, warning or error (in red)."""
    _notify("message", {"text": str(text), "level": level})


def output(text):
    """A line in the Output panel."""
    _notify("output", {"text": str(text)})


def text():
    """The text of the current file."""
    return (_request("editor/text") or {}).get("text", "")


def replace_selection(text):
    """Replaces what is selected (each selection) by text."""
    _request("editor/replace_selection", {"text": text})


def insert(text):
    """Inserts text at the cursor (each cursor)."""
    _request("editor/insert", {"text": text})


def set_text(text):
    """Replaces the whole text of the current file (undone by Ctrl+Z)."""
    _request("editor/set_text", {"text": text})


def open(path, line=None):
    """Opens a file, at a line (from 1)."""
    _request("open", {"path": str(path), "line": line})


def pick(items, title=""):
    """A list to choose from (strings, or (label, detail)): the index chosen, None on Esc."""
    entries = [{"label": i[0], "detail": i[1]} if isinstance(i, (tuple, list)) else {"label": str(i)} for i in items]
    return _request("pick", {"title": title, "items": entries})


def input(title, value=""):
    """A text typed by the user, None on Esc."""
    return _request("input", {"title": title, "value": value})


def run(command):
    """Runs a command of Florm by its id (those of [keys] in settings.toml: "save", "goto_line"...)."""
    _request("run", {"command": command})


def terminal(cmd):
    """Runs a command line in a terminal of Florm."""
    _request("terminal", {"cmd": cmd})


# ------------------------------------------------------------------ the loop

def _handle(msg):
    method = msg.get("method")
    params = msg.get("params") or {}
    rid = msg.get("id")
    try:
        result = None
        if method == "initialize":
            info.update(params)
        elif method == "command":
            f = _commands.get(params.get("id"))
            if f is None:
                raise FlormError(f"no function for the command {params.get('id')!r} (@florm.command)")
            result = f(Context(params.get("context")))
        elif method == "event":
            for f in _events.get(params.get("name"), []):
                f(Context(params.get("context")))
        if rid is not None:
            _send({"jsonrpc": "2.0", "id": rid, "result": result if isinstance(result, (str, int, float, bool, list, dict)) else None})
    except Exception as e:
        tb = traceback.format_exc()
        if rid is not None:
            _send({"jsonrpc": "2.0", "id": rid, "error": {"code": -32000, "message": f"{type(e).__name__}: {e}", "data": {"traceback": tb}}})
        else:
            _notify("message", {"text": f"{type(e).__name__}: {e}", "level": "error"})
            sys.stderr.write(tb)


def main():
    """Answers Florm until it stops the extension."""
    threading.Thread(target=_reader, daemon=True).start()
    while True:
        msg = _incoming.get()
        if msg is None:
            return
        # a command may wait for an answer of Florm (pick, input): one at a time, in order
        _handle(msg)
