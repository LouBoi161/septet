Septet 0.2.0 brings Claude into the apps.

- **Claude chat panel** (speech bubble at the top right, or Ctrl+Shift+K): Claude works in all seven
  apps with you. It runs their commands, looks at the result and can undo. It also makes SVGs, layouts,
  PDFs and motion graphics from code and places them in an app. You choose the model and effort, and
  you can add your own skills, plugins and MCP servers. It needs
  [Claude Code](https://code.claude.com/docs/en/setup), installed and signed in with your own Claude
  subscription; Septet starts it and never sees your sign-in. Septet asks you first before Claude saves
  or exports outside its working folder, changes settings, prints or signs.
- **Deleting layers** with the Delete key or the right-click menu now works in Vectorcraft, Photocraft,
  Designcraft, Pdfcraft (new: deletes a PDF layer and its content) and Filmcraft (track headers and
  Essential Graphics layers). In Filmcraft, Delete in the Timeline no longer deletes the item still
  selected in the Project panel.
- **One Septet at a time**: starting Septet again, or opening a file from the file manager, hands the
  files to the Septet that is already running.
- Filmcraft on Windows and Linux: three default shortcuts no longer share a key (Ctrl+9, Ctrl+Shift+M,
  Ctrl+T).

**Downloads:** Windows installer and portable zip, Linux AppImage and tar.gz, macOS for Apple Silicon
and Intel. The builds are unsigned — see the README for the one-time "run anyway" step on Windows and
macOS. `SHA256SUMS.txt` lists the checksums.

Septet's own code is under the PolyForm Noncommercial License 1.0.0; the bundled apps are MIT OR
Apache-2.0 by the ArtCraft team. Septet is not affiliated with the ArtCraft Team or Adobe.
