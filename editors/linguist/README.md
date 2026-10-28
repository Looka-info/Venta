# Official GitHub Linguist Submission Package

This folder contains the complete definitions for officially registering the **Vanta** programming language in [github-linguist/linguist](https://github.com/github-linguist/linguist).

---

## Files Included

1. **`languages.yml`**: Official YAML definition for GitHub Linguist:
   - Name: `Vanta` (alias: `venta`)
   - Extensions: `.va`, `.vanta`
   - Scope: `source.vanta`
   - Color: `#7C3AED` (Violet)
2. **`sample.va`**: Verified real-world Vanta source sample code demonstrating structs, methods, mutable references, and standard I/O.
3. **`../vscode/syntaxes/vanta.tmLanguage.json`**: Standard TextMate syntax highlighting grammar.

---

## Submission Procedure to GitHub Linguist

To have Vanta globally recognized natively on GitHub without `.gitattributes` overrides:

1. Fork [https://github.com/github-linguist/linguist](https://github.com/github-linguist/linguist).
2. Append the contents of `editors/linguist/languages.yml` to `lib/linguist/languages.yml`.
3. Add `editors/linguist/sample.va` to `samples/Vanta/sample.va`.
4. Add `editors/vscode/syntaxes/vanta.tmLanguage.json` to `vendor/grammars/`.
5. Run Linguist tests (`bundle exec rake test`).
6. Submit Pull Request: `Add support for Vanta language`.
