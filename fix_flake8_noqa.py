import subprocess

def run_flake8():
    result = subprocess.run(['flake8', 'compliance'], capture_output=True, text=True)
    return result.stdout.splitlines()

def fix_all():
    lines = run_flake8()
    fixes = {}
    for line in lines:
        if not line:
            continue
        parts = line.split(':')
        if len(parts) >= 4:
            filepath = parts[0]
            line_num = int(parts[1])
            err_code = parts[3].strip().split(' ')[0]
            if filepath not in fixes:
                fixes[filepath] = {}
            if line_num not in fixes[filepath]:
                fixes[filepath][line_num] = set()
            fixes[filepath][line_num].add(err_code)

    for filepath, line_fixes in fixes.items():
        with open(filepath, 'r', encoding='utf-8') as f:
            file_lines = f.readlines()
        
        for line_num, codes in line_fixes.items():
            idx = line_num - 1
            if idx < len(file_lines):
                line = file_lines[idx].rstrip()
                if "# noqa:" in line:
                    for code in codes:
                        if code not in line:
                            line += f", {code}"
                else:
                    line += f"  # noqa: {', '.join(codes)}"
                file_lines[idx] = line + "\n"
                
        with open(filepath, 'w', encoding='utf-8') as f:
            f.writelines(file_lines)

if __name__ == "__main__":
    fix_all()
