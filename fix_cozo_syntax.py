import re

with open('src/store/cozo.rs', 'r') as f:
    lines = f.readlines()

new_lines = []
skip = False
for i, line in enumerate(lines):
    if "let events: Vec<DomainEvent> = evts" in line and "let events: Vec<DomainEvent> = evts\n" == line:
        continue # this was the broken orphaned line... wait, let's just use regex.

