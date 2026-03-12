import re
with open("src/domain/analyze.rs", "r") as f: text = f.read()
text = re.sub(r'fn is_public.*?\n}\n', '', text, flags=re.DOTALL)
text = re.sub(r'fn type_to_string.*?\n}\n', '', text, flags=re.DOTALL)
text = re.sub(r'fn is_option_type.*?\n}\n', '', text, flags=re.DOTALL)
with open("src/domain/analyze.rs", "w") as f: f.write(text)
