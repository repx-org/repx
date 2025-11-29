import argparse
import ast
import json
import re
import shutil
import sys

import bashlex

SHELL_BUILTINS = {
    "alias",
    "bg",
    "bind",
    "break",
    "builtin",
    "caller",
    "cd",
    "command",
    "compgen",
    "complete",
    "compopt",
    "continue",
    "declare",
    "dirs",
    "disown",
    "echo",
    "enable",
    "eval",
    "exec",
    "exit",
    "export",
    "fc",
    "fg",
    "getopts",
    "hash",
    "help",
    "history",
    "jobs",
    "kill",
    "let",
    "local",
    "logout",
    "mapfile",
    "popd",
    "printf",
    "pushd",
    "pwd",
    "read",
    "readarray",
    "readonly",
    "return",
    "set",
    "shift",
    "shopt",
    "source",
    "suspend",
    "test",
    "times",
    "trap",
    "type",
    "typeset",
    "ulimit",
    "umask",
    "unalias",
    "unset",
    "wait",
    "true",
    "false",
    ".",
    ":",
    "[",
    "[[",
    "]]",
    "}",
    "{",
    "if",
    "then",
    "else",
    "elif",
    "fi",
    "while",
    "do",
    "done",
    "for",
    "in",
    "case",
    "esac",
    "function",
}

DEBUG = True


def debug_print(msg):
    if DEBUG:
        print(f"[DEBUG] {msg}", file=sys.stderr)


class CommandVisitor(bashlex.ast.nodevisitor):
    def __init__(self):
        self.commands = set()

    def visitcommand(self, _, parts):
        for part in parts:
            if part.kind == "word":
                word = part.word
                if word.startswith('"') and word.endswith('"'):
                    word = word[1:-1]
                if word.startswith("'") and word.endswith("'"):
                    word = word[1:-1]

                if not word.startswith("$"):
                    self.commands.add(word)
                break
        return True


def extract_from_ast_file(filepath):
    """
    Parses the OSH AST text file using robust Regex to handle
    nested structures and multiline content strings.
    """
    with open(filepath, "r") as f:
        data = f.read()

    defined_functions = set()
    func_pattern = re.compile(r"\((?:command\.)?ShFunction\s+.*?name:(\w+)", re.DOTALL)
    for match in func_pattern.finditer(data):
        defined_functions.add(match.group(1))

    debug_print(f"Defined functions in script: {defined_functions}")

    source_lines = set()

    regex_pattern = r"""
        \(command\.Simple             # Start of block
        (?:(?!\(command\.).)*?        # Guard: Don't cross into new command
        blame_tok:\(Token             # Find token block
        (?:(?!\(command\.).)*?        # Guard
        line:\(SourceLine             # Must be a SourceLine struct
        (?:(?!\(command\.).)*?        # Guard
        content:"                     # Anchor to content tag
        ((?:[^"\\]|\\.)*)             # CAPTURE: The inner content string
        "
    """

    pattern = re.compile(regex_pattern, re.VERBOSE | re.DOTALL)

    matches = pattern.findall(data)

    for raw_content in matches:
        try:
            reconstructed_literal = f'"{raw_content}"'
            line_text = ast.literal_eval(reconstructed_literal)

            if line_text.strip():
                source_lines.add(line_text)
        except Exception as e:
            debug_print(f"Failed to unescape content: {raw_content[:20]}... Error: {e}")

    debug_print(f"Extracted {len(source_lines)} unique source lines.")
    return defined_functions, source_lines


def analyze_line(line):
    commands = set()
    clean_line = line.strip()

    try:
        trees = bashlex.parse(clean_line)
        visitor = CommandVisitor()
        for tree in trees:
            visitor.visit(tree)
        if visitor.commands:
            return visitor.commands
    except Exception:
        pass

    return commands


def analyze(ast_file):
    try:
        defined_funcs, lines = extract_from_ast_file(ast_file)
    except FileNotFoundError:
        return {"error": f"AST file not found: {ast_file}"}, 1

    found_commands = set()

    for line in lines:
        cmds = analyze_line(line)
        found_commands.update(cmds)

    final_commands = set()
    for cmd in found_commands:
        cmd = cmd.strip()
        if not cmd:
            continue
        if cmd in SHELL_BUILTINS:
            continue
        if cmd in defined_funcs:
            continue
        if cmd.startswith("$"):
            continue
        if "=" in cmd:
            continue
        if cmd.startswith("-"):
            continue
        if cmd.startswith("${"):
            continue

        final_commands.add(cmd)

    debug_print(f"--- [ANALYSIS] Found static commands: {final_commands}")

    results = {"ok": [], "missing": [], "ignored": []}

    for cmd in sorted(final_commands):
        if cmd.startswith("/"):
            results["ok"].append({"cmd": cmd, "type": "absolute"})
            continue

        path = shutil.which(cmd)
        if path:
            results["ok"].append({"cmd": cmd, "path": path})
        else:
            results["missing"].append(cmd)

    return results, (1 if results["missing"] else 0)


def main():
    parser = argparse.ArgumentParser(
        description="Analyze shell dependencies via OSH AST"
    )
    parser.add_argument("ast_file", help="Path to the OSH AST text file")
    parser.add_argument("--json", help="Path to write JSON output", default=None)
    args = parser.parse_args()

    results, exit_code = analyze(args.ast_file)

    if args.json:
        with open(args.json, "w") as f:
            json.dump(results, f, indent=2)

    if results.get("missing"):
        print(
            "\n\033[31m[ERROR] Missing dependencies detected:\033[0m", file=sys.stderr
        )
        for m in results["missing"]:
            print(f" - {m}", file=sys.stderr)
        print(
            "\nPlease add these to 'runDependencies' in your stage definition.",
            file=sys.stderr,
        )
    elif "error" in results:
        print(f"[FATAL] {results['error']}", file=sys.stderr)
    else:
        print(
            f"\033[32m[OK] Dependencies verified ({len(results['ok'])} found).\033[0m",
            file=sys.stderr,
        )

    sys.exit(exit_code)


if __name__ == "__main__":
    main()
