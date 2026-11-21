#!/usr/bin/env python3
"""
check_test_case_req_links.py

Scans all Rust test cases in `src` and `tests` directories and verifies if each test case
has a linkage to a software/system requirement (`[utest->swreq~...]` or `[itest->swreq~...]`).

Usage:
    python3 scripts/check_test_case_req_links.py [options]

Options:
    --unlinked-only    Show only test cases that are missing requirement linkage.
    --linked-only      Show only test cases that have requirement linkage.
    --summary-only     Show only summary statistics.
    --markdown         Output test cases as a formatted Markdown table.
    --json             Output analysis in JSON format.
    --root <dir>       Specify root directory of the repository (default: current working directory or repo root).
"""

import os
import sys
import re
import json
import argparse


def find_repo_root(start_dir):
    current = os.path.abspath(start_dir)
    while current != os.path.dirname(current):
        if os.path.exists(os.path.join(current, "Cargo.toml")) or os.path.exists(os.path.join(current, ".git")):
            return current
        current = os.path.dirname(current)
    return os.path.abspath(start_dir)


def check_file_test_cases(filepath, repo_root):
    rel_file_path = os.path.relpath(filepath, repo_root)
    dir_name = os.path.dirname(rel_file_path)
    file_name = os.path.basename(rel_file_path)

    try:
        with open(filepath, "r", encoding="utf-8", errors="replace") as f:
            lines = f.readlines()
    except Exception as e:
        print(f"Error reading {filepath}: {e}", file=sys.stderr)
        return []

    tests = []
    for i, line in enumerate(lines):
        line_str = line.strip()
        # Detect #[test] or #[tokio::test] attributes
        if line_str in ["#[test]", "#[tokio::test]"] or line_str.startswith("#[test") or line_str.startswith("#[tokio::test"):
            fn_name = None
            fn_line_idx = i
            # Search downwards for the test function signature
            for k in range(i + 1, min(len(lines), i + 35)):
                fn_match = re.search(r'(?:pub\s+)?(?:async\s+)?fn\s+([a-zA-Z0-9_]+)', lines[k])
                if fn_match:
                    fn_name = fn_match.group(1)
                    fn_line_idx = k
                    break

            if not fn_name:
                continue

            # Search window for requirement tags [utest->swreq~...] or [itest->swreq~...]
            start_search = max(0, i - 15)
            end_search = min(len(lines), fn_line_idx + 10)
            window = "".join(lines[start_search:end_search])
            tags = re.findall(r'\[(?:itest|utest)->(swreq~[^\]]+)\]', window)
            req_tags = sorted(list(set(tags)))

            tests.append({
                "name": fn_name,
                "relative_path": dir_name,
                "file_name": file_name,
                "full_relative_path": rel_file_path,
                "line": i + 1,
                "fn_line": fn_line_idx + 1,
                "reqs": req_tags,
                "is_linked": len(req_tags) > 0,
            })

    return tests


def scan_repository(repo_root):
    target_dirs = ["src", "tests"]
    all_tests = []
    files_scanned = 0

    for tdir in target_dirs:
        abs_tdir = os.path.join(repo_root, tdir)
        if not os.path.exists(abs_tdir):
            continue

        for root, _, files in os.walk(abs_tdir):
            for file in sorted(files):
                if file.endswith(".rs"):
                    files_scanned += 1
                    filepath = os.path.join(root, file)
                    file_tests = check_file_test_cases(filepath, repo_root)
                    all_tests.extend(file_tests)

    return all_tests, files_scanned


def main():
    parser = argparse.ArgumentParser(description="Check requirement linkage for all Rust test cases in src/ and tests/.")
    parser.add_argument("--unlinked-only", action="store_true", help="Display only test cases missing requirement linkage.")
    parser.add_argument("--linked-only", action="store_true", help="Display only test cases with requirement linkage.")
    parser.add_argument("--summary-only", action="store_true", help="Display only summary statistics.")
    parser.add_argument("--markdown", action="store_true", help="Output results in Markdown table format.")
    parser.add_argument("--json", action="store_true", help="Output results in JSON format.")
    parser.add_argument("--root", type=str, default=None, help="Root directory of repository.")

    args = parser.parse_args()

    repo_root = args.root if args.root else find_repo_root(os.getcwd())
    all_tests, files_scanned = scan_repository(repo_root)

    total_tests = len(all_tests)
    linked_tests = [t for t in all_tests if t["is_linked"]]
    unlinked_tests = [t for t in all_tests if not t["is_linked"]]
    linked_count = len(linked_tests)
    unlinked_count = len(unlinked_tests)
    coverage_pct = (linked_count / total_tests * 100.0) if total_tests > 0 else 0.0

    # Filter tests according to options
    if args.unlinked_only:
        display_tests = unlinked_tests
    elif args.linked_only:
        display_tests = linked_tests
    else:
        display_tests = all_tests

    if args.json:
        result_data = {
            "summary": {
                "files_scanned": files_scanned,
                "total_test_cases": total_tests,
                "linked_test_cases": linked_count,
                "unlinked_test_cases": unlinked_count,
                "linkage_percentage": round(coverage_pct, 2),
            },
            "test_cases": display_tests,
        }
        print(json.dumps(result_data, indent=2))
        return

    if not args.summary_only:
        if args.markdown:
            print("| Test Function Name | Relative Path | File Name | Line | Status | Linked Requirement(s) |")
            print("|---|---|---|---|---|---|")
            for t in display_tests:
                status_str = "Linked" if t["is_linked"] else "**Unlinked**"
                req_str = ", ".join(t["reqs"]) if t["is_linked"] else "-"
                print(f"| `{t['name']}` | `{t['relative_path']}` | `{t['file_name']}` | {t['fn_line']} | {status_str} | `{req_str}` |")
            print()
        else:
            print(f"{'Test Function Name':<65} | {'Relative Path':<35} | {'File Name':<40} | {'Status':<12}")
            print("-" * 160)
            for t in display_tests:
                status_str = f"LINKED ({', '.join(t['reqs'])})" if t["is_linked"] else "UNLINKED"
                print(f"{t['name']:<65} | {t['relative_path']:<35} | {t['file_name']:<40} | {status_str:<12}")
            print("-" * 160)
            print()

    # Print Summary Statistics
    print("=" * 60)
    print("                TEST CASE LINKAGE STATISTICS                ")
    print("=" * 60)
    print(f"Rust Source Files Scanned : {files_scanned}")
    print(f"Total Test Cases Checked  : {total_tests}")
    print(f"Linked Test Cases         : {linked_count}")
    print(f"Unlinked Test Cases       : {unlinked_count}")
    print(f"Requirement Linkage Coverage: {coverage_pct:.2f}%")
    print("=" * 60)


if __name__ == "__main__":
    main()
