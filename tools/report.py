"""Write the interesting part of a build log as a pull-request comment.

GitHub keeps only a handful of annotations per check run, so a throwaway
debug workflow reports through a comment instead: the lines a failure is
described by, plus the lines that follow them (a panic message's body sits
on the lines after `panicked at`). Not used by the shipped workflows.

    python tools/report.py t.txt
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
        # A panic message continues on the following lines.
        if "panicked" in line or "failures:" in line:
            picked.extend(lines[index + 1 : index + 12])
            index += 12
            continue
    index += 1
body = "\n".join(picked) or text[-4000:]
print("```text")
print(body[:14000])
print("```")
