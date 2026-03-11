import { useState, useEffect, useCallback } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  readTextFile,
  writeTextFile,
  readDir,
} from "@tauri-apps/plugin-fs";
import { listen } from "@tauri-apps/api/event";
import { join } from "@tauri-apps/api/path";

type OpenedFile = {
  path: string;
  content: string;
};

type TreeEntry = {
  name: string;
  path: string;
  isDirectory: boolean;
  children?: TreeEntry[];
};

async function loadTree(dirPath: string): Promise<TreeEntry[]> {
  const entries = await readDir(dirPath);
  const result: TreeEntry[] = [];
  for (const entry of entries) {
    const path = await join(dirPath, entry.name);
    const node: TreeEntry = {
      name: entry.name,
      path,
      isDirectory: entry.isDirectory,
    };
    if (entry.isDirectory) {
      try {
        node.children = await loadTree(path);
        node.children.sort((a, b) => {
          if (a.isDirectory !== b.isDirectory)
            return a.isDirectory ? -1 : 1;
          return a.name.localeCompare(b.name, undefined, { sensitivity: "base" });
        });
      } catch {
        node.children = [];
      }
    }
    result.push(node);
  }
  result.sort((a, b) => {
    if (a.isDirectory !== b.isDirectory) return a.isDirectory ? -1 : 1;
    return a.name.localeCompare(b.name, undefined, { sensitivity: "base" });
  });
  return result;
}

function FileTree({
  entries,
  onOpenFile,
  rootPath,
}: {
  entries: TreeEntry[];
  onOpenFile: (path: string) => void;
  rootPath: string;
}) {
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
  const toggle = (path: string) => {
    setCollapsed((c) => ({ ...c, [path]: !c[path] }));
  };
  return (
    <ul style={{ listStyle: "none", margin: 0, paddingLeft: "0.75rem" }}>
      {entries.map((e) => (
        <li key={e.path}>
          {e.isDirectory ? (
            <>
              <button
                type="button"
                onClick={() => toggle(e.path)}
                style={{
                  background: "none",
                  border: "none",
                  color: "#ccc",
                  cursor: "pointer",
                  padding: "2px 4px",
                  fontSize: "13px",
                  display: "flex",
                  alignItems: "center",
                  gap: "4px",
                  width: "100%",
                  textAlign: "left",
                }}
              >
                <span style={{ width: 14 }}>
                  {collapsed[e.path] ? "▶" : "▼"}
                </span>
                <span>{e.name}</span>
              </button>
              {!collapsed[e.path] && e.children && e.children.length > 0 && (
                <FileTree
                  entries={e.children}
                  onOpenFile={onOpenFile}
                  rootPath={rootPath}
                />
              )}
            </>
          ) : (
            <button
              type="button"
              onClick={() => onOpenFile(e.path)}
              style={{
                background: "none",
                border: "none",
                color: "#b0b0b0",
                cursor: "pointer",
                padding: "2px 4px 2px 18px",
                fontSize: "13px",
                display: "block",
                width: "100%",
                textAlign: "left",
              }}
            >
              {e.name}
            </button>
          )}
        </li>
      ))}
    </ul>
  );
}

export default function App() {
  const [projectRoot, setProjectRoot] = useState<string | null>(null);
  const [tree, setTree] = useState<TreeEntry[]>([]);
  const [file, setFile] = useState<OpenedFile | null>(null);
  const [isDirty, setIsDirty] = useState(false);
  const [warning, setWarning] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const setWarningForPath = useCallback((path: string) => {
    let ext = "";
    const lastSlash = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
    const lastDot = path.lastIndexOf(".");
    if (lastDot > lastSlash) {
      ext = path.slice(lastDot + 1).toLowerCase();
    }
    if (ext && ext !== "v" && ext !== "sv") {
      setWarning(
        `Opened non-Verilog file ".${ext}". This IDE is optimized for .v / .sv, but you can still edit this file.`
      );
    } else {
      setWarning(null);
    }
  }, []);

  const handleOpenFolder = useCallback(async () => {
    setError(null);
    try {
      const selected = await open({
        directory: true,
        multiple: false,
      });
      if (!selected || Array.isArray(selected)) return;
      setProjectRoot(selected);
      const entries = await loadTree(selected);
      setTree(entries);
      setFile(null);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setError(`Could not open folder: ${message}`);
    }
  }, []);

  const handleOpenFile = useCallback(async () => {
    setError(null);
    try {
      const selected = await open({
        multiple: false,
      });
      if (!selected || Array.isArray(selected)) return;
      setWarningForPath(selected);
      const content = await readTextFile(selected);
      setFile({ path: selected, content });
      setIsDirty(false);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setError(`Could not open file: ${message}`);
    }
  }, [setWarningForPath]);

  const openFileByPath = useCallback(
    async (path: string) => {
      setError(null);
      try {
        setWarningForPath(path);
        const content = await readTextFile(path);
        setFile({ path, content });
        setIsDirty(false);
      } catch (e) {
        const message = e instanceof Error ? e.message : String(e);
        setError(`Could not open file: ${message}`);
      }
    },
    [setWarningForPath]
  );

  const handleSave = useCallback(async () => {
    if (!file) return;
    setError(null);
    try {
      await writeTextFile(file.path, file.content);
      setIsDirty(false);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setError(`Could not save file: ${message}`);
    }
  }, [file]);

  useEffect(() => {
    const unsubs: (() => void)[] = [];
    listen("menu-open-folder", () => {
      handleOpenFolder();
    }).then((u) => unsubs.push(u));
    listen("menu-open-file", () => {
      handleOpenFile();
    }).then((u) => unsubs.push(u));
    listen("menu-save", () => {
      handleSave();
    }).then((u) => unsubs.push(u));
    return () => {
      unsubs.forEach((fn) => fn());
    };
  }, [handleOpenFolder, handleOpenFile, handleSave]);

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100vh" }}>
      <header
        style={{
          display: "flex",
          alignItems: "center",
          gap: "0.5rem",
          padding: "0.5rem 0.75rem",
          borderBottom: "1px solid #333",
          background: "#121212",
          color: "#f0f0f0",
        }}
      >
        <strong>Circuit Scope</strong>
        <span style={{ marginLeft: "auto", opacity: 0.7, fontSize: "0.85rem" }}>
          {file ? file.path : projectRoot ?? "No folder open"}
          {file && isDirty ? " *" : ""}
        </span>
      </header>
      <div style={{ flex: 1, display: "flex", overflow: "hidden" }}>
        {projectRoot && (
          <aside
            style={{
              width: 260,
              minWidth: 200,
              borderRight: "1px solid #333",
              background: "#1a1a1a",
              color: "#e0e0e0",
              overflow: "auto",
              padding: "0.5rem 0",
            }}
          >
            <div
              style={{
                padding: "0.25rem 0.75rem",
                fontSize: "11px",
                textTransform: "uppercase",
                opacity: 0.7,
              }}
            >
              {projectRoot
                ? projectRoot.replace(/^.*[/\\]/, "") || "Folder"
                : "Folder"}
            </div>
            <FileTree
              entries={tree}
              onOpenFile={openFileByPath}
              rootPath={projectRoot}
            />
          </aside>
        )}
        <main
          style={{
            flex: 1,
            display: "flex",
            flexDirection: "column",
            overflow: "hidden",
          }}
        >
          {error && (
            <div
              style={{
                padding: "0.5rem 0.75rem",
                background: "#3a2020",
                color: "#ff8a8a",
                borderBottom: "1px solid #663333",
                fontSize: "0.85rem",
              }}
            >
              {error}
            </div>
          )}
          {warning && (
            <div
              style={{
                padding: "0.5rem 0.75rem",
                background: "#302000",
                color: "#ffd27f",
                borderBottom: "1px solid #664000",
                fontSize: "0.85rem",
              }}
            >
              {warning}
            </div>
          )}
          <textarea
            spellCheck={false}
            style={{
              flex: 1,
              border: "none",
              outline: "none",
              padding: "0.75rem",
              fontFamily: "monospace",
              fontSize: "14px",
              background: "#181818",
              color: "#f0f0f0",
              resize: "none",
            }}
            disabled={!file}
            value={file?.content ?? ""}
            onChange={(e) => {
              if (!file) return;
              setFile({ ...file, content: e.target.value });
              setIsDirty(true);
            }}
          />
        </main>
      </div>
    </div>
  );
}
