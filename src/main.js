const tauri = window.__TAURI__;
const invoke = tauri.core.invoke;
const convertFileSrc = tauri.core.convertFileSrc;
const listen = tauri.event.listen;

const filenameEl = document.querySelector("#filename");
const emptyEl = document.querySelector("#empty");
const docEl = document.querySelector("#doc");
const readerEl = document.querySelector("#reader");
const findbar = document.querySelector("#findbar");
const findInput = document.querySelector("#find-input");
const toastEl = document.querySelector("#toast");

let currentPath = "";
let currentDirectory = "";
let requestId = 0;
let toastTimer = 0;

document.querySelector("#open-file").addEventListener("click", chooseFile);
document.querySelector("#empty-open").addEventListener("click", chooseFile);
document.querySelector("#make-default").addEventListener("click", makeDefault);
document.querySelector("#find-toggle").addEventListener("click", openFind);
document.querySelector("#find-close").addEventListener("click", closeFind);
document.querySelector("#find-prev").addEventListener("click", () => find(true));
findbar.addEventListener("submit", (event) => {
  event.preventDefault();
  find(false);
});

document.addEventListener("keydown", (event) => {
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "o") {
    event.preventDefault();
    chooseFile();
  }
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "f") {
    event.preventDefault();
    openFind();
  }
  if (event.key === "Escape" && !findbar.hidden) {
    closeFind();
  }
});

docEl.addEventListener("click", onDocumentClick);

listen("open-file", (event) => {
  if (typeof event.payload === "string" && event.payload) {
    showPath(event.payload);
  }
});

listen("file-changed", (event) => {
  if (event.payload && event.payload === currentPath) {
    const scroll = readerEl.scrollTop;
    showPath(currentPath, scroll);
  }
});

listen("tauri://drag-enter", () => {
  document.body.classList.add("dropping");
});

listen("tauri://drag-leave", () => {
  document.body.classList.remove("dropping");
});

listen("tauri://drag-drop", (event) => {
  document.body.classList.remove("dropping");
  const paths = event.payload?.paths ?? [];
  const preferred =
    paths.find((path) => /\.(md|markdown|mdown|mkd)$/i.test(path)) ?? paths[0];
  if (preferred) {
    showPath(preferred);
  }
});

invoke("pending_open").then((path) => {
  if (path) {
    showPath(path);
  }
});

async function chooseFile() {
  const selected = await invoke("plugin:dialog|open", {
    options: {
      title: "Open Markdown",
      multiple: false,
      directory: false,
      defaultPath: currentDirectory || undefined,
      filters: [
        {
          name: "Markdown",
          extensions: ["md", "markdown", "mdown", "mkd"],
        },
      ],
    },
  });
  if (typeof selected === "string" && selected) {
    showPath(selected);
  }
}

async function showPath(path, restoreScroll) {
  const id = ++requestId;
  try {
    const doc = await invoke("open_path", { path });
    if (id !== requestId) {
      return;
    }
    render(doc);
    if (typeof restoreScroll === "number") {
      readerEl.scrollTop = restoreScroll;
    } else {
      readerEl.scrollTop = 0;
    }
  } catch (error) {
    if (id === requestId) {
      toast(errorMessage(error));
    }
  }
}

function render(doc) {
  currentPath = doc.path;
  currentDirectory = doc.directory;
  filenameEl.textContent = doc.name;
  filenameEl.title = doc.path;
  docEl.innerHTML = doc.html;
  for (const image of docEl.querySelectorAll("img")) {
    const src = image.getAttribute("src") || "";
    if (src.startsWith("md-local:")) {
      image.src = convertFileSrc(decodeURIComponent(src.slice("md-local:".length)));
    }
  }
  emptyEl.hidden = true;
  docEl.hidden = false;
}

async function onDocumentClick(event) {
  const link = event.target.closest("a");
  if (!link || !docEl.contains(link)) {
    return;
  }
  const href = link.getAttribute("href");
  if (!href) {
    return;
  }
  event.preventDefault();
  if (href.startsWith("#")) {
    document.getElementById(decodeURIComponent(href.slice(1)))?.scrollIntoView({ block: "start" });
    return;
  }
  if (/^(https?:|mailto:)/i.test(href)) {
    try {
      await invoke("open_external", { url: href });
    } catch (error) {
      toast(errorMessage(error));
    }
    return;
  }
  if (!currentPath) {
    return;
  }
  const hashIndex = href.indexOf("#");
  const hash = hashIndex >= 0 ? href.slice(hashIndex) : "";
  const target = hashIndex >= 0 ? href.slice(0, hashIndex) : href;
  if (!target) {
    document.getElementById(decodeURIComponent(hash.slice(1)))?.scrollIntoView({ block: "start" });
    return;
  }
  try {
    const resolved = await invoke("resolve_href", { currentFile: currentPath, href: target });
    await showPath(resolved);
    if (hash.length > 1) {
      document.getElementById(decodeURIComponent(hash.slice(1)))?.scrollIntoView({ block: "start" });
    }
  } catch (error) {
    toast(errorMessage(error));
  }
}

async function makeDefault() {
  try {
    const message = await invoke("register_default");
    toast(message);
  } catch (error) {
    toast(errorMessage(error));
  }
}

function openFind() {
  findbar.hidden = false;
  findInput.focus();
  findInput.select();
}

function closeFind() {
  findbar.hidden = true;
  window.getSelection()?.removeAllRanges();
}

function find(backwards) {
  const query = findInput.value;
  if (!query) {
    return;
  }
  window.find(query, false, backwards, true);
}

function toast(message) {
  toastEl.textContent = message;
  toastEl.hidden = false;
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => {
    toastEl.hidden = true;
  }, 7000);
}

function errorMessage(error) {
  if (typeof error === "string") {
    return error;
  }
  if (error && typeof error === "object" && typeof error.message === "string") {
    return error.message;
  }
  return "Something went wrong.";
}
