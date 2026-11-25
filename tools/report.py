"""Write the interesting part of a build log as a pull-request comment.

Throwaway debug aid: GitHub keeps only a handful of annotations per check
run, so a debug workflow reports through a comment instead. Not shipped.
"""
import re
import sys

path = sys.argv[1] if len(sys.argv) > 1 else "t.txt"
text = open(path, encoding="utf-8", errors="replace").read()
text = re.sub(r"\x1b\[[0-9;]*m", "", text)
lines = text.splitlines()
keys = ("panicked", "failures:", "test result", "error[", "error:", "assertion")
picked: list[str] = []
index = 0
while index < len(lines) and len(picked) < 120:
    line = lines[index]
    if any(key in line for key in keys):
        picked.append(line)
        if "panicked" in line or "failures:" in line:
            picked.extend(lines[index + 1 : index + 12])
            index += 12
            continue
    index += 1

picked = [line if len(line) <= 240 else line[:240] + " …" for line in picked]
body = "\n".join(picked) or text[-4000:]
print("```text")
print(body[:14000])
print("```")
