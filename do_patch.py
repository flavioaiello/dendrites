import os
os.system('git restore src/store/cozo.rs')
os.system('python3 patch_cozo_schema.py')
os.system('python3 strict_patch_cozo.py')
os.system('cargo check')
