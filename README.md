# MD Reader

A small Windows app for reading Markdown. Install it, set it as the default app, and double-click any `.md` file to open it here.

![MD Reader ready for a file](docs/welcome.png)

![A Markdown file open in MD Reader](docs/reading.png)

## Download and install

1. Open the [latest release](https://github.com/tpwatson/md-reader/releases/latest).
2. Download `MD Reader_0.1.0_x64-setup.exe`.
3. Run the installer. It installs for the current Windows user and does not ask for an administrator password.
4. Open **MD Reader** from the Start menu.
5. Click **Use for .md files**.
6. In the Windows list, check the Markdown extensions and save.

After that, double-clicking a `.md`, `.markdown`, `.mdown`, or `.mkd` file opens it in MD Reader. If the app is already open, the new file replaces the one you are reading.

You can also right-click a Markdown file, choose **Open with**, and pick **MD Reader**.

## Reading

| Action | What it does |
| --- | --- |
| Open, or Ctrl+O | Pick a Markdown file |
| Drop a file on the window | Open that file |
| Ctrl+F | Find text in the document |
| Click a link to another `.md` file | Open it in MD Reader |
| Click a web link | Open it in your browser |

Images stored next to the Markdown file are shown in the page. If you save the file from another editor, MD Reader refreshes.

## Build from source

You need Rust, Node.js, and the Microsoft C++ build tools (WebView2 is already on Windows 10 and 11).

```
npm install
npm run tauri dev
```

The development build can open files. Windows will not use it as the default app. To build the installer:

```
npm run tauri build
```

The installer is written to `src-tauri/target/release/bundle/nsis/`.
