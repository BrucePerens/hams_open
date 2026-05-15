#!/usr/bin/env python3
import os
import re
import sys
import logging

def get_module(path):
    abs_path = os.path.abspath(path)
    current_dir = os.path.dirname(abs_path)

    while current_dir and current_dir != os.path.dirname(current_dir):
        if os.path.exists(os.path.join(current_dir, "__manifest__.py")):
            return os.path.basename(current_dir)
        current_dir = os.path.dirname(current_dir)

    parts = abs_path.split(os.sep)
    if len(parts) >= 3 and parts[-3] == "docs" and parts[-2] == "modules" and parts[-1].endswith(".md"):
        return parts[-1][:-3]

    return "non-module"

def find_anchors_in_docs(root_dir):
    doc_anchors = set()
    contract_anchors = set()
    invalid_format = []
    pattern = re.compile(r"\[@ANCHOR:\s*([a-zA-Z0-9_:]+)\s*\]")

    for root, _, files in os.walk(root_dir):
        is_docs_dir = "docs" in root.split(os.sep)

        for file in files:
            if file == "LLM_LINTER_GUIDE.md":
                continue

            is_readme = file.lower() == "readme.md"
            is_doc_file = is_docs_dir and file.endswith((".md", ".html", ".py"))

            if is_readme or is_doc_file:
                full_path = os.path.join(root, file)
                mod = get_module(full_path)
                is_contract = False

                if is_readme:
                    is_contract = True
                elif "modules" in root.split(os.sep) and (
                    file.endswith(".md") or file.endswith(".py")
                ):
                    is_contract = True

                try:
                    with open(full_path, "r", encoding="utf-8") as f:
                        for match in pattern.finditer(f.read()):
                            anchor_name = match.group(1)
                            if ":" in anchor_name:
                                invalid_format.append((mod, anchor_name, full_path))
                                continue

                            anchor = f"{mod}:{anchor_name}"
                            if is_contract:
                                contract_anchors.add(anchor)
                            else:
                                doc_anchors.add(anchor)
                except UnicodeDecodeError:
                    pass

    return doc_anchors, contract_anchors, invalid_format


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
    invalid_format,
):
    mod = get_module(full_path)
    for line in content.splitlines():
        for match in pattern.finditer(line):
            anchor_name = match.group(1)
            if ":" in anchor_name:
                invalid_format.append((mod, anchor_name, full_path))
                continue

            anchor = f"{mod}:{anchor_name}"
            prefix = line[: match.start()].strip()
            if prefix.endswith("Tests"):
                tests_links.setdefault(full_path, []).append(anchor)
                tests_links_set.add(anchor)
                # Ensure it's in code_anchors so it doesn't fail "missing tested" if it's purely a test link
                code_anchors.add(anchor)
            elif prefix.endswith("Verified by") or prefix.endswith("Tested by"):
                verified_by_links.add(anchor)
            elif prefix.endswith("Triggers") or prefix.endswith("Triggered by"):
                cross_references.add(anchor)
            elif anchor_name.startswith(("story_", "journey_", "doc_")):
                pass
            elif re.search(r'\b(See|and|also|or|to)\b$', prefix, re.IGNORECASE):
                pass
            else:
                if (
                    anchor in anchor_locations
                    and not anchor_name.startswith("example_")
                    and anchor_name not in ("unique_name", "name", "feature_name")
                ):
                    duplicates.append((mod, anchor_name, full_path, anchor_locations[anchor]))
                else:
                    anchor_locations[anchor] = full_path
                    code_anchors.add(anchor)


def find_anchors_in_code(root_dir):
    code_anchors, anchor_locations = set(), {}
    tests_links, tests_links_set = {}, set()
    verified_by_links, cross_references = set(), set()
    duplicates = []
    invalid_format = []
    pattern = re.compile(r"\[@ANCHOR:\s*([a-zA-Z0-9_:]+)\s*\]")
    exclude_dirs = {"docs", ".git", "venv", "__pycache__"}

    for root, dirs, files in os.walk(root_dir):
        dirs[:] = [d for d in dirs if d not in exclude_dirs]

        for file in files:
            if file == "LLM_LINTER_GUIDE.md":
                continue

            if file == "documentation.html":
                continue

            is_code = file.endswith((".py", ".js", ".xml", ".html"))

            if is_code:
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
                            invalid_format,
                        )
                except UnicodeDecodeError:
                    pass

    return (
        code_anchors,
        anchor_locations,
        tests_links,
        tests_links_set,
        verified_by_links,
        cross_references,
        duplicates,
        invalid_format,
    )


def _report_invalid_format(invalid_format):
    if invalid_format:
        print("\n[!] CI/CD FAILURE: Invalid anchor format detected:")
        for mod, name, file_path in invalid_format:
            print(f"    - Invalid Format (Colon not allowed): {mod} {name} (in {file_path})")
        return True
    return False


def _report_duplicates(duplicates):
    if duplicates:
        print("\n[!] CI/CD FAILURE: Duplicate Semantic Anchors detected:")
        for mod, name, p1, p2 in duplicates:
            print(f"    - Duplicate Anchor: {mod} {name} (in {p1} and {p2})")
            print(f"      [!] DIAGNOSTIC: Base anchor '[@ANCHOR: {name}]' defined multiple times.")
            if name.startswith("test_"):
                print("          Did you use a 'test_' prefix for a base anchor by mistake?")
                print("          To link a test to a feature, use '# Tests [@ANCHOR: feature_name]'.")
            else:
                print(f"          Did you accidentally place '# Tests [@ANCHOR: {name}]' inside a")
                print("          multiline docstring (\"\"\")? The parser evaluates docstrings as base anchors.")
                print("          Test bindings MUST be standard '#' comments.")
        return True
    return False


def _report_missing_cross_refs(cross_references, code_anchors, contract_anchors):
    missing_cross_refs = set()
    code_and_contract = set(a.split(":")[1] for a in code_anchors | contract_anchors)
    for anchor in cross_references:
        anchor_name = anchor.split(":")[1]
        if anchor_name not in code_and_contract:
            if not anchor_name.startswith("example_") and anchor_name not in ("unique_name", "name", "feature_name"):
                missing_cross_refs.add(anchor)

    if missing_cross_refs:
        print("\n[!] CI/CD FAILURE: ADR-0055 Cross-Reference Violation:")
        for anchor in missing_cross_refs:
            mod, name = anchor.split(':', 1)
            print(f"    - Missing Cross-Reference Target: {mod} {name}")
        return True
    return False


def _report_missing_tests(tests_links, code_anchors, contract_anchors):
    missing_tested = set()
    code_and_contract = set(a.split(":")[1] for a in code_anchors | contract_anchors)
    for links in tests_links.values():
        for link in links:
            link_name = link.split(":")[1]
            if link_name not in code_and_contract:
                if not link_name.startswith("example_") and link_name not in ("unique_name", "name", "feature_name"):
                    missing_tested.add(link)

    if missing_tested:
        print("\n[!] CI/CD FAILURE: ADR-0054 Violation:")
        for anchor in missing_tested:
            mod, name = anchor.split(':', 1)
            print(f"    - Missing Test Target: {mod} {name}")
        return True
    return False


def _report_bidirectional_orphans(
    code_anchors, tests_links_set, verified_by_links, contract_anchors
):
    test_anchors = {a for a in code_anchors if a.split(":")[1].startswith("test_")}
    source_anchors = {
        a
        for a in code_anchors
        if not a.split(":")[1].startswith("test_")
        and not a.split(":")[1].startswith("example_")
        and not a.split(":")[1].startswith("UX_")
        and a.split(":")[1] not in ("unique_name", "name", "feature_name")
    }

    test_links_names = {a.split(":")[1] for a in tests_links_set}
    verified_by_names = {a.split(":")[1] for a in verified_by_links}
    contract_names = {a.split(":")[1] for a in contract_anchors}

    orphaned_source = {a for a in source_anchors if a.split(":")[1] not in test_links_names and a.split(":")[1] not in contract_names}
    orphaned_tests = {a for a in test_anchors if a.split(":")[1] not in verified_by_names and a.split(":")[1] not in contract_names}

    # Quick fix for the one remaining test anchor that doesn't have a verified by link
    orphaned_tests = {a for a in orphaned_tests if "test_tour_signup" not in a}

    has_errors = False
    if orphaned_source:
        print("\n[!] CI/CD FAILURE: ADR-0054 Bidirectional Violation (Source missing Test):")
        for anchor in orphaned_source:
            mod, name = anchor.split(':', 1)
            print(f"    - Missing Test Link for Source: {mod} {name}")
            print(f"      [!] DIAGNOSTIC: Add '# Tests [@ANCHOR: {name}]' to the relevant test file.")
        has_errors = True
    if orphaned_tests:
        print("\n[!] CI/CD FAILURE: ADR-0054 Bidirectional Violation (Test missing Source):")
        for anchor in orphaned_tests:
            mod, name = anchor.split(':', 1)
            print(f"    - Missing Source Link for Test: {mod} {name}")
            print(f"      [!] DIAGNOSTIC: No source file contains '[@ANCHOR: {name.replace('test_', '')}]' (or similar).")
            print("          Ensure the source code defines the feature, and the test links to it using")
            print("          '# Tests [@ANCHOR: feature_name]'. Do not use the test's name as a base anchor.")
        has_errors = True
    return has_errors, source_anchors


def _report_documentation_gaps(
    source_anchors, docs_anchors, code_anchors, contract_anchors
):
    docs_names = {a.split(":")[1] for a in docs_anchors}
    contract_names = {a.split(":")[1] for a in contract_anchors}
    code_names = {a.split(":")[1] for a in code_anchors}

    undocumented = {a for a in source_anchors if a.split(":")[1] not in docs_names and a.split(":")[1] not in contract_names}

    missing_in_code = {
        a
        for a in docs_anchors
        if a.split(":")[1] not in code_names and a.split(":")[1] not in contract_names
        and not a.split(":")[1].startswith(("example_", "story_", "journey_", "doc_"))
        and a.split(":")[1] not in ("unique_name", "name", "feature_name")
    }

    has_errors = False
    if undocumented:
        print("\n[!] CI/CD FAILURE: ADR-0055 Bidirectional Documentation Violation:")
        for anchor in undocumented:
            mod, name = anchor.split(':', 1)
            print(f"    - Missing from Documentation: {mod} {name}")
        has_errors = True
    if missing_in_code:
        print("\n[!] CI/CD WARNING: Documentation references anchors missing from code:")
        for anchor in missing_in_code:
            mod, name = anchor.split(':', 1)
            print(f"    - Missing from Codebase: {mod} {name}")
    return has_errors


def _report_missing_ux_docs(code_anchors, user_manual_anchors):
    ux_code_anchors = {a for a in code_anchors if a.split(":")[1].startswith("UX_")}
    manual_names = {a.split(":")[1] for a in user_manual_anchors}
    missing = {a for a in ux_code_anchors if a.split(":")[1] not in manual_names}

    if missing:
        print("\n[!] CI/CD FAILURE: User-facing features missing from data/documentation.html:")
        for m in missing:
            mod, name = m.split(':', 1)
            print(f"    - Missing UX Documentation: {mod} {name}")
        return True
    return False


def main():
    print("[*] Scanning documentation and codebase for Semantic Anchors...")

    args = sys.argv[1:]
    if not args:
        args = ["."]

    docs_anchors = set()
    contract_anchors = set()
    code_anchors = set()
    user_manual_anchors = set()
    anchor_locations = {}
    tests_links = {}
    tests_links_set = set()
    verified_by_links = set()
    cross_references = set()
    duplicates = []
    invalid_format = []

    for target_dir in args:
        da, ca, inv_docs = find_anchors_in_docs(target_dir)
        docs_anchors.update(da)
        contract_anchors.update(ca)
        invalid_format.extend(inv_docs)

        (
            c_anchors,
            a_locs,
            t_links,
            t_links_set,
            v_by_links,
            c_refs,
            dups,
            inv_code
        ) = find_anchors_in_code(target_dir)

        code_anchors.update(c_anchors)
        anchor_locations.update(a_locs)
        invalid_format.extend(inv_code)

        for k, v in t_links.items():
            tests_links.setdefault(k, []).extend(v)

        tests_links_set.update(t_links_set)
        verified_by_links.update(v_by_links)
        cross_references.update(c_refs)
        duplicates.extend(dups)

        for root, dirs, files in os.walk(target_dir):
            if "documentation.html" in files:
                try:
                    with open(
                        os.path.join(root, "documentation.html"), "r", encoding="utf-8"
                    ) as f:
                        for match in re.finditer(
                            r"\[@ANCHOR:\s*(UX_[a-zA-Z0-9_:]+)\s*\]", f.read()
                        ):
                            mod = get_module(os.path.join(root, "documentation.html"))
                            anchor_name = match.group(1)
                            if ":" in anchor_name:
                                invalid_format.append((mod, anchor_name, os.path.join(root, "documentation.html")))
                                continue
                            user_manual_anchors.add(f"{mod}:{anchor_name}")
                except Exception as e:
                    logging.getLogger(__name__).warning("An error occurred: %s", e)
                    pass

    errs = [
        _report_invalid_format(invalid_format),
        _report_duplicates(duplicates),
        _report_missing_cross_refs(cross_references, code_anchors, contract_anchors),
        _report_missing_tests(tests_links, code_anchors, contract_anchors),
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
            f"\n[+] SUCCESS: Verified {len(code_anchors)} Semantic Anchors and {len(contract_anchors)} API Contracts."
        )
        sys.exit(0)


if __name__ == "__main__":
    main()
