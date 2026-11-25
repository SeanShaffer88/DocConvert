"""Print one annotation line with the interesting part of a build log.

GitHub keeps only a handful of annotations per check run, so a step that
needs to report a failure has to fit it into one line instead of
annotating every line of the log. Used by the throwaway ci-debug
workflow; harmless everywhere else.
"""
import re
import sys

path = sys.argv[1] if len(sys.argv) > 1 else "t.txt"
text = open(path, encoding="utf-8", errors="replace").read()
text = re.sub(r"\x1b\[[0-9;]*m", "", text)
keys = ("panicked", "error", "failures:", "assertion", "left:", "right:", "test result")
pick = [line for line in text.splitlines() if any(key in line for key in keys)]
payload = " || ".join(pick[:30]) or text[-1500:]
print("::error::" + payload.replace("%", "%25").replace("\r", "")[:6000])
