# Vanta Language Support for VS Code

Official extension providing language support, syntax highlighting, and code configuration for the **Vanta Systems Programming Language** (`.va`, `.vanta`).

---

## Features

- **Syntax Highlighting:** Complete grammar coverage for keywords (`fn`, `let`, `mut`, `struct`, `impl`, `trait`, etc.), types, lifetimes, operators, string escapes, and numeric literals.
- **Bracket Matching & Auto-closing:** Automatic closing pairs for `{}`, `[]`, `()`, and `""`.
- **Comment Toggling:** Standard line (`//`) and block (`/* */`) comments.

---

## Local Installation

To use this extension locally in VS Code:

### Linux / macOS:
```bash
ln -s "$(pwd)/editors/vscode" ~/.vscode/extensions/vanta-lang
```

### Windows (PowerShell):
```powershell
New-Item -ItemType SymbolicLink -Path "$HOME\.vscode\extensions\vanta-lang" -Target "$PWD\editors\vscode"
```

Restart VS Code or reload the window (`Ctrl+Shift+P` -> `Developer: Reload Window`), and any `.va` or `.vanta` file will automatically receive full syntax highlighting!
