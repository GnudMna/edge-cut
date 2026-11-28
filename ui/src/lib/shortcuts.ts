// キーボードショートカットの判定

export type Shortcut = "open" | "batchTrim" | "paste" | "save" | "copy" | "clear" | "cancel";

interface KeyLike {
  key: string;
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  metaKey: boolean;
}

/** キー入力に対応するショートカットを返す (無ければ `null`) */
export function shortcutFor(e: KeyLike): Shortcut | null {
  if (e.altKey || e.metaKey) return null;
  const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
  if (e.ctrlKey) {
    if (key === "o") return e.shiftKey ? "batchTrim" : "open";
    if (e.shiftKey) return null;
    if (key === "v") return "paste";
    if (key === "s") return "save";
    if (key === "c") return "copy";
    return null;
  }
  if (e.shiftKey) return null;
  if (key === "Delete") return "clear";
  if (key === "Escape") return "cancel";
  return null;
}

/** 文字入力中か (入力欄ではコピー・貼り付け・Delete を入力欄に任せる) */
export function isTextInput(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  if (target instanceof HTMLTextAreaElement) return true;
  return target instanceof HTMLInputElement && ["text", "search", "number", "email", "url"].includes(target.type);
}
