with open("src/store/cozo.rs", "r") as f:
    lines = f.readlines()

new_lines = []
skip = False
for i, line in enumerate(lines):
    if line.strip() == "let api_endpoints_rows = self.db.run_script(":
        # let's look behind to see if we're near the second one
        if lines[i-4].strip() == ".collect();":
            skip = True
    
    if skip and "}).collect();" in line:
        skip = False
        continue

    if skip:
        continue
    new_lines.append(line)

with open("src/store/cozo.rs", "w") as f:
    f.writelines(new_lines)
