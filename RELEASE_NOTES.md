Septet 0.3.0 adds Wordcraft, one look for every app, local image generation, and installing,
uninstalling and updating from inside Septet.

- **Wordcraft**, the eighth app: word processing for DOCX, ODT, RTF, Markdown, HTML and more, as a
  tab like the others. Claude can write and format documents in it.
- **Apps and updates** (Home › Apps): install and uninstall each app. An uninstalled app leaves
  Home, the + menu, *Open in…* and Claude's tools; your files and its settings stay for when you
  install it again. The same page updates Septet: it looks for a new version once a day (you can
  turn that off), downloads the file for your system, checks its SHA-256 and puts it in place;
  *Restart Now* starts the new version. Works for the AppImage, the Linux tar.gz, the Windows
  installer and portable zip, and Septet.app. This is the first version with it, so updating *to*
  0.3.0 is still a download by hand.
- **Appearance** (Home › Appearance, or the window menu): one brightness for Septet and every app —
  Darkest, Dark, Light, Lightest or the desktop's setting — plus an accent colour, one UI size for
  everything, and *More contrast*. The apps' own theme menus open this dialog.
- **Generative Fill and Generate Image** in Photocraft (Edit menu, and the selection tools'
  right-click menu), with Qwen Image 2.1 in your own [ComfyUI](https://www.comfy.org/) on your
  computer. Septet starts ComfyUI when needed, frees its graphics memory when idle, and makes room
  first if the card is full. Claude can generate and edit images with it, too. Septet doesn't ship
  ComfyUI or the model: install them yourself.
- **All apps updated** to their latest releases: Photocraft 0.15.1, Vectorcraft and Effectcraft
  0.16.0, Pdfcraft 0.13.0, Lightcraft 0.13.0, Designcraft and Filmcraft 0.12.0, Wordcraft 0.10.0.
- **Smaller download**: the apps share one copy of their fonts instead of one each.
- **Graphics problems**: when the graphics driver resets, Septet saves every app's recovery copy
  and starts again, and the apps offer your work back.

**Downloads:** Windows installer and portable zip, Linux AppImage and tar.gz, macOS for Apple Silicon
and Intel. The builds are unsigned — see the README for the one-time "run anyway" step on Windows and
macOS. `SHA256SUMS.txt` lists the checksums.

Septet's own code is under the PolyForm Noncommercial License 1.0.0; the bundled apps are MIT OR
Apache-2.0 by the ArtCraft team. Septet is not affiliated with the ArtCraft Team or Adobe.
