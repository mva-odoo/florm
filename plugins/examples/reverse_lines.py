import sys

text = sys.stdin.read()
end = "\n" if text.endswith("\n") else ""
sys.stdout.write("\n".join(reversed(text.splitlines())) + end)
