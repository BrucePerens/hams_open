#!/usr/bin/env python3
import os
import re
import sys

def get_module(path):
    abs_path = os.path.abspath(path)
    current_dir = os.path.dirname(abs_path)

    # 1. Try to find __manifest__.py (Odoo Standard)
    while current_dir and current_dir != os.path.dirname(current_dir):
        if os.path.exists(os.path.join(current_dir, "__manifest__.py")):
            return os.path.basename(current_dir)
        current_dir = os.path.dirname(current_dir)

    # 2. Fallback for global docs/modules/...
    parts = abs_path.split(os.sep)
    if len(parts) >= 3 and parts[-3] == "docs" and parts[-2] == "modules" and parts[-1].endswith(".md"):
        return parts[-1][:-3]

    # 3. Fallback: Repo top-level directory mapping
    cdir = os.path.dirname(abs_path)
    repo_root = None
    while cdir and cdir != os.path.dirname(cdir):
        if os.path.exists(os.path.join(cdir, "tools", "verify_anchors.py")) or os.path.exists(os.path.join(cdir, ".git")):
            repo_root = cdir
            break
        cdir = os.path.dirname(cdir)

    if repo_root:
        rel_path = os.path.relpath(abs_path, repo_root)
        parts = rel_path.split(os.sep)
        if len(parts) > 1 and parts[0] not in ("docs", "tools", "scripts", ".git", "venv", "__pycache__"):
            return parts[0]

    return "global"


def find_anchors_in_docs(root_dir, repo_root):
    doc_anchors = {}
    contract_anchors = {}
    pattern = re.compile(r"\[@ANCHOR:\s*([a-zA-Z0-9_:]+)\s*\]")
    exclude_dirs = {"tools", "scripts", "hams_community", "hams_com", ".git", "venv", "__pycache__"}

    for root, dirs, files in os.walk(root_dir):
        dirs[:] = [d for d in dirs if d not in exclude_dirs]
        is_docs_dir = "docs" in root.split(os.sep)

        for file in files:
            if file == "LLM_LINTER_GUIDE.md":
                continue

            is_readme = file.lower() == "readme.md"
            is_doc_file = is_docs_dir and file.endswith((".md", ".html", ".py"))

            if is_readme or is_doc_file:
                full_path = os.path.join(root, file)
                rel_path = os.path.relpath(full_path, repo_root)
                mod = get_module(full_path)
                is_contract = is_readme or ("modules" in root.split(os.sep) and file.endswith((".md", ".py")))

                try:
                    with open(full_path, "r", encoding="utf-8") as f:
                        for line_num, line in enumerate(f, 1):
                            for match in pattern.finditer(line):
                                anchor_name = match.group(1)
                                explicit_mod = mod
                                if ":" in anchor_name:
                                    explicit_mod, anchor_name = anchor_name.split(":", 1)

                                anchor = f"{explicit_mod}:{anchor_name}"
                                loc_str = f"./{rel_path}:{line_num}"

                                if is_contract:
                                    contract_anchors.setdefault(anchor, []).append(loc_str)
                                else:
                                    doc_anchors.setdefault(anchor, []).append(loc_str)
                except UnicodeDecodeError:
                    continue

    return doc_anchors, contract_anchors


def _process_file_for_anchors(
    full_path,
    content,
    pattern,
    code_anchors,
    anchor_locations,
    tests_links,
    tests_links_set,
    verified_by_links,
    cross_references,
    duplicates,
    repo_root,
):
    mod = get_module(full_path)
    rel_path = os.path.relpath(full_path, repo_root)

    for line_num, line in enumerate(content.splitlines(), 1):
        matches = list(pattern.finditer(line))
        if not matches:
            continue

        first_prefix = line[: matches[0].start()].strip()
        loc_str = f"./{rel_path}:{line_num}"

        for match in matches:
            anchor_name = match.group(1)
            explicit_mod = mod

            if ":" in anchor_name:
                explicit_mod, anchor_name = anchor_name.split(":", 1)

            anchor = f"{explicit_mod}:{anchor_name}"

            if first_prefix.endswith("Tests"):
                tests_links.setdefault(full_path, []).append((anchor, line_num))
                tests_links_set.setdefault(anchor, []).append(loc_str)
                code_anchors.setdefault(anchor, []).append(loc_str)
            elif first_prefix.endswith("Verified by") or first_prefix.endswith("Tested by"):
                verified_by_links.setdefault(anchor, []).append(loc_str)
            elif first_prefix.endswith("Triggers") or first_prefix.endswith("Triggered by"):
                cross_references.setdefault(anchor, []).append(loc_str)
            elif anchor_name.startswith(("story_", "journey_", "doc_")):
                pass
            elif re.search(r'\b(See|and|also|or|to)\b$', first_prefix, re.IGNORECASE):
                pass
            else:
                base_name = anchor.split(":")[1]
                if (
                    anchor in anchor_locations
                    and not base_name.startswith("example_")
                    and base_name not in ("unique_name", "name", "feature_name")
                ):
                    duplicates.append((anchor, loc_str, anchor_locations[anchor]))
                else:
                    anchor_locations.setdefault(anchor, []).append(loc_str)
                    code_anchors.setdefault(anchor, []).append(loc_str)


def find_anchors_in_code(root_dir, repo_root):
    code_anchors, anchor_locations = {}, {}
    tests_links, tests_links_set = {}, {}
    verified_by_links, cross_references = {}, {}
    duplicates = []
    pattern = re.compile(r"\[@ANCHOR:\s*([a-zA-Z0-9_:]+)\s*\]")
    exclude_dirs = {"docs", ".git", "venv", "__pycache__", "tools", "scripts", "hams_community", "hams_com"}

    for root, dirs, files in os.walk(root_dir):
        dirs[:] = [d for d in dirs if d not in exclude_dirs]

        for file in files:
            if file == "LLM_LINTER_GUIDE.md" or file == "documentation.html":
                continue

            if file.endswith((".py", ".js", ".xml", ".html")):
                full_path = os.path.join(root, file)
                try:
                    with open(full_path, "r", encoding="utf-8") as f:
                        _process_file_for_anchors(
                            full_path,
                            f.read(),
                            pattern,
                            code_anchors,
                            anchor_locations,
                            tests_links,
                            tests_links_set,
                            verified_by_links,
                            cross_references,
                            duplicates,
                            repo_root,
                        )
                except UnicodeDecodeError:
                    continue

    return (
        code_anchors,
        anchor_locations,
        tests_links,
        tests_links_set,
        verified_by_links,
        cross_references,
        duplicates,
    )


def _report_duplicates(duplicates):
    if duplicates:
        print("\n[!] CI/CD FAILURE: Duplicate Semantic Anchors detected:")
        for anchor, current_loc, prior_locs in duplicates:
            print(f"    - Duplicate Anchor Found: '{anchor}'")
            print(f"      Current Location: {current_loc}")
            print("      Prior Definition(s):")
            for prior in prior_locs:
                print(f"        -> {prior}")

            base_name = anchor.split(":")[1]
            if base_name.startswith("test_"):
                print("      [!] DIAGNOSTIC: Do not use a 'test_' prefix for a base code anchor definition.")
            else:
                msg_formatting = {"prefix": "[@ANCHOR", "label": "feature"}
                print(f"      [!] DIAGNOSTIC: Did you accidentally wrap a base macro '{msg_formatting['prefix']}: {msg_formatting['label']}]' inside python multiline docstrings?")
        return True
    return False


def _report_missing_cross_refs(cross_references, code_anchors, contract_anchors):
    has_errors = False
    all_known_anchors = set(code_anchors.keys()) | set(contract_anchors.keys())

    for anchor, source_locs in cross_references.items():
        if anchor not in all_known_anchors:
            base_name = anchor.split(":")[1]
            if base_name.startswith("example_") or base_name in ("unique_name", "name", "feature_name"):
                continue

            if not has_errors:
                print("\n[!] CI/CD FAILURE: ADR-0055 Strict Module-Bound Cross-Reference Violation:")
                has_errors = True

            print(f"    - Missing Cross-Reference Target: '{anchor}'")
            print("      Triggered from locations:")
            for loc in source_locs:
                print(f"        -> {loc}")
            print("      [!] DIAGNOSTIC: Use 'target_module:anchor_name' syntax to cross boundaries intentionally.")
    return has_errors


def _report_missing_tests(tests_links, code_anchors, contract_anchors, repo_root):
    has_errors = False
    all_known_anchors = set(code_anchors.keys()) | set(contract_anchors.keys())

    for filepath, links in tests_links.items():
        rel_path = os.path.relpath(filepath, repo_root)
        for anchor, line in links:
            if anchor not in all_known_anchors:
                base_name = anchor.split(":")[1]
                if base_name.startswith("example_") or base_name in ("unique_name", "name", "feature_name"):
                    continue

                if not has_errors:
                    print("\n[!] CI/CD FAILURE: ADR-0054 Strict Module-Bound Linkage Violation:")
                    has_errors = True
                print(f"    - Broken '# Tests' Binding: Target '{anchor}' does not exist in any codebase directory.")
                print(f"      Location: ./{rel_path}:{line}")
    return has_errors


def _report_bidirectional_orphans(code_anchors, tests_links_set, verified_by_links, contract_anchors):
    has_errors = False
    all_contracts = set(contract_anchors.keys())

    test_anchors = {a: locs for a, locs in code_anchors.items() if a.split(":")[1].startswith("test_")}
    source_anchors = {
        a: locs for a, locs in code_anchors.items()
        if not a.split(":")[1].startswith("test_")
        and not a.split(":")[1].startswith("example_")
        and not a.split(":")[1].startswith("UX_")
        and a.split(":")[1] not in ("unique_name", "name", "feature_name")
    }

    orphaned_source = {a: locs for a, locs in source_anchors.items() if a not in tests_links_set and a not in all_contracts}
    orphaned_tests = {a: locs for a, locs in test_anchors.items() if a not in verified_by_links and a not in all_contracts}
    orphaned_tests = {a: locs for a, locs in orphaned_tests.items() if "test_tour_signup" not in a.split(":")[1]}

    if orphaned_source:
        print("\n[!] CI/CD FAILURE: ADR-0054 Bidirectional Disconnect (Source Missing Test Link):")
        for anchor, locs in orphaned_source.items():
            print(f"    - Code Feature '{anchor}' has no active test linkage coverage.")
            print("      Feature Definition Locations:")
            for loc in locs:
                print(f"        -> {loc}")
            print(f"      [!] DIAGNOSTIC: Append a tracking line '# Tests [@ANCHOR: {anchor.split(':')[1]}]' inside the corresponding test file.")
        has_errors = True

    if orphaned_tests:
        print("\n[!] CI/CD FAILURE: ADR-0054 Bidirectional Disconnect (Test Missing Feature Reference):")
        for anchor, locs in orphaned_tests.items():
            print(f"    - Test Logic Target '{anchor}' has no inverse implementation link.")
            print("      Test Definition Locations:")
            for loc in locs:
                print(f"        -> {loc}")
            print("      [!] DIAGNOSTIC: Ensure your production files define the feature, and add '# Tested by' anchors where appropriate.")
        has_errors = True

    return has_errors, source_anchors


def _report_documentation_gaps(source_anchors, docs_anchors, code_anchors, contract_anchors):
    has_errors = False
    all_contracts = set(contract_anchors.keys())

    undocumented = {a: locs for a, locs in source_anchors.items() if a not in docs_anchors and a not in all_contracts}
    missing_in_code = {
        a: locs for a, locs in docs_anchors.items()
        if a not in code_anchors and a not in all_contracts
        and not a.split(":")[1].startswith(("example_", "story_", "journey_", "doc_"))
        and a.split(":")[1] not in ("unique_name", "name", "feature_name")
    }

    if undocumented:
        print("\n[!] CI/CD FAILURE: ADR-0055 Documentation Coverage Gap Detected:")
        for anchor, locs in undocumented.items():
            print(f"    - Code Feature '{anchor}' is completely missing from documentation manuals.")
            print("      Feature Definition Locations:")
            for loc in locs:
                print(f"        -> {loc}")
        has_errors = True

    if missing_in_code:
        print("\n[!] CI/CD WARNING: Documentation references anchors missing from codebase modules:")
        for anchor, locs in missing_in_code.items():
            print(f"    - Reference Target '{anchor}' is missing from operational source code.")
            print("      Referenced inside Manual Files:")
            for loc in locs:
                print(f"        -> {loc}")
            h = {"t": "[@ANCHOR"}
            print(f"      [!] DIAGNOSTIC: If this targets an external domain context, explicitly structure it as '{h['t']}: module_name:{anchor.split(':')[1]}]'")
    return has_errors


def _report_missing_ux_docs(code_anchors, user_manual_anchors):
    ux_code_anchors = {a: locs for a, locs in code_anchors.items() if a.split(":")[1].startswith("UX_")}
    has_errors = False

    for anchor, locs in ux_code_anchors.items():
        if anchor not in user_manual_anchors:
            if not has_errors:
                print("\n[!] CI/CD FAILURE: User-Facing Portal Features missing from module documentation.html index:")
                has_errors = True
            print(f"    - Missing User Manual Item: '{anchor}'")
            print("      Declared in layout code files:")
            for loc in locs:
                print(f"        -> {loc}")
            print(f"      [!] DIAGNOSTIC: Append a container item '<span style=\"display:none;\">[@ANCHOR: {anchor.split(':')[1]}]</span>' to your module's data/documentation.html file.")
    return has_errors


def main():
    print("[*] Scanning documentation and codebase for Semantic Anchors...")
    repo_root = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))

    args = sys.argv[1:]
    if not args:
        args = ["."]

    docs_anchors, contract_anchors = {}, {}
    code_anchors, anchor_locations = {}, {}
    tests_links, tests_links_set = {}, {}
    verified_by_links, cross_references = {}, {}
    user_manual_anchors = set()
    duplicates = []

    for target_dir in args:
        da, ca = find_anchors_in_docs(target_dir, repo_root)
        for k, v in da.items(): docs_anchors.setdefault(k, []).extend(v)
        for k, v in ca.items(): contract_anchors.setdefault(k, []).extend(v)

        (
            c_anchors,
            a_locs,
            t_links,
            t_links_set,
            v_by_links,
            c_refs,
            dups
        ) = find_anchors_in_code(target_dir, repo_root)

        for k, v in c_anchors.items(): code_anchors.setdefault(k, []).extend(v)
        for k, v in a_locs.items(): anchor_locations.setdefault(k, []).extend(v)
        for k, v in t_links.items(): tests_links.setdefault(k, []).extend(v)
        for k, v in t_links_set.items(): tests_links_set.setdefault(k, []).extend(v)
        for k, v in v_by_links.items(): verified_by_links.setdefault(k, []).extend(v)
        for k, v in c_refs.items(): cross_references.setdefault(k, []).extend(v)
        duplicates.extend(dups)

        for root, dirs, files in os.walk(target_dir):
            dirs[:] = [d for d in dirs if d not in {"tools", "scripts", "hams_community", "hams_com", ".git", "venv", "__pycache__"}]
            if "documentation.html" in files:
                full_doc_path = os.path.join(root, "documentation.html")
                try:
                    with open(full_doc_path, "r", encoding="utf-8") as f:
                        for match in re.finditer(r"\[@ANCHOR:\s*(UX_[a-zA-Z0-9_:]+)\s*\]", f.read()):
                            mod = get_module(full_doc_path)
                            anchor_name = match.group(1)
                            if ":" in anchor_name:
                                mod, anchor_name = anchor_name.split(":", 1)
                            user_manual_anchors.add(f"{mod}:{anchor_name}")
                except (OSError, UnicodeDecodeError):
                    continue

    errs = [
        _report_duplicates(duplicates),
        _report_missing_cross_refs(cross_references, code_anchors, contract_anchors),
        _report_missing_tests(tests_links, code_anchors, contract_anchors, repo_root),
        _report_missing_ux_docs(code_anchors, user_manual_anchors),
    ]

    bidi_err, source_anchors = _report_bidirectional_orphans(
        code_anchors, tests_links_set, verified_by_links, contract_anchors
    )
    errs.append(bidi_err)
    errs.append(
        _report_documentation_gaps(
            source_anchors, docs_anchors, code_anchors, contract_anchors
        )
    )

    if any(errs):
        sys.exit(1)
    else:
        print(
            f"\n[+] SUCCESS: Verified {len(code_anchors)} Semantic Anchors and {len(contract_anchors)} API Contracts. (Module Bounds Secure & Explicit Targeting Profile Operational)"
        )
        sys.exit(0)


if __name__ == "__main__":
    main()
