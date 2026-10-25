Septet 0.4.0 adds Deckcraft for presentations, makes drag & drop work on GNOME, and puts light and
dark in one place.

- **Deckcraft**, the ninth app: presentations for PPTX, POTX, PPSX and .deckcraft, with slide
  shows, animations and transitions, as a tab like the others. The slide show runs full screen in
  the Septet window; with a second screen the Presenter View opens full screen there. Claude can
  build and edit presentations in it.
- **Drag & drop on GNOME**: dropping files from the file manager onto Septet didn't work at all
  under GNOME. Now it does, and every file goes to the right place: dropped on an app that takes it
  (its own files, or a picture into the open document) it lands there; anything else opens in the
  app made for it — a PPTX in Deckcraft, a CR3 in Lightcraft, and so on.
- **One appearance button**: light and dark is now a button next to Claude in the tab strip, for
  all apps at once; the apps' own light/dark switches are gone.
- **No more "not responding"**: Deckcraft's Open, Save As, Export and Insert Picture dialogs no
  longer freeze the window while they are open.
- **Chat focus**: clicking Claude's message field while you type in a slide moves the keyboard to
  the chat.

**Downloads:** Windows installer and portable zip, Linux AppImage and tar.gz, macOS for Apple Silicon
and Intel. The builds are unsigned — see the README for the one-time "run anyway" step on Windows and
macOS. `SHA256SUMS.txt` lists the checksums.

Septet's own code is under the PolyForm Noncommercial License 1.0.0; the bundled apps are MIT OR
Apache-2.0 by the ArtCraft team. Septet is not affiliated with the ArtCraft Team or Adobe.
