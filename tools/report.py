"""Write the interesting part of a build log as a pull-request comment.

GitHub keeps only a handful of annotations per check run, so a throwaway
debug workflow reports through a comment instead: one fenced block with
the lines a failure is described by. Not used by the shipped workflows.
"""
import re
import sys

path = sys.argv[1] if len(sys.argv) > 1 else "t.txt"
text = open(path, encoding="utf-8", errors="replace").read()
text = re.sub(r"\x1b\[[0-9;]*m", "", text)
keys = ("panicked", "error", "failures:", "assertion", "left:", "right:", "test result", "warning:")
pick = [line for line in text.splitlines() if any(key in line for key in keys)]
body = "\n".join(pick[:80]) or text[-4000:]
print("```text")
print(body[:12000])
print("```")
