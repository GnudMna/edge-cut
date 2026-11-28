// メニュー・ポップオーバー・ダイアログ

import { type ReactNode, useEffect, useRef } from "react";

import { Icon, type IconName } from "./Icon";

/** 外側のクリックと Esc で閉じる */
function useDismiss(open: boolean, onClose: () => void, ref: React.RefObject<HTMLElement | null>) {
  useEffect(() => {
    if (!open) return;
    const onPointer = (e: PointerEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("pointerdown", onPointer, true);
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("blur", onClose);
    return () => {
      window.removeEventListener("pointerdown", onPointer, true);
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("blur", onClose);
    };
  }, [open, onClose, ref]);
}

// ======================================================================
// ポップオーバー
// ======================================================================
interface PopoverProps {
  open: boolean;
  onClose: () => void;
  anchor: ReactNode;
  children: ReactNode;
  align?: "start" | "end";
  className?: string;
}

/** 基準の部品の下に開く浮いた枠 */
export function Popover({ open, onClose, anchor, children, align = "start", className }: PopoverProps) {
  const ref = useRef<HTMLDivElement>(null);
  useDismiss(open, onClose, ref);
  return (
    <div className="popover-anchor" ref={ref}>
      {anchor}
      {open && <div className={`popover popover-${align} ${className ?? ""}`}>{children}</div>}
    </div>
  );
}

// ======================================================================
// メニュー
// ======================================================================
export interface MenuItem {
  icon?: IconName;
  label: string;
  hint?: string;
  onSelect?: () => void;
  disabled?: boolean;
  checked?: boolean;
  separator?: boolean;
  note?: string;
}

interface MenuProps {
  items: MenuItem[];
  onClose: () => void;
}

export function MenuList({ items, onClose }: MenuProps) {
  return (
    <div className="menu" role="menu">
      {items.map((item, i) =>
        item.separator ? (
          <div key={`sep-${i}`} className="menu-separator" role="separator" />
        ) : item.onSelect ? (
          <button
            key={item.label}
            type="button"
            role={item.checked === undefined ? "menuitem" : "menuitemradio"}
            aria-checked={item.checked}
            className="menu-item"
            disabled={item.disabled}
            title={item.hint}
            onClick={() => {
              item.onSelect?.();
              onClose();
            }}
          >
            <span className="menu-icon">
              {item.checked ? <Icon name="check" size={14} /> : item.icon && <Icon name={item.icon} size={15} />}
            </span>
            <span className="menu-label">{item.label}</span>
            {item.note && <span className="menu-note">{item.note}</span>}
          </button>
        ) : (
          <div key={item.label} className="menu-caption">
            {item.label}
          </div>
        ),
      )}
    </div>
  );
}

// ======================================================================
// ダイアログ
// ======================================================================
interface DialogProps {
  title: string;
  icon?: IconName;
  tone?: "default" | "danger" | "success";
  children: ReactNode;
  actions: ReactNode;
  /** Esc・背景クリックで閉じる。`undefined` のときは閉じられない */
  onClose?: (() => void) | undefined;
  wide?: boolean;
}

export function Dialog({ title, icon, tone = "default", children, actions, onClose, wide }: DialogProps) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    ref.current?.querySelector<HTMLButtonElement>(".dialog-actions .btn-primary, .dialog-actions .btn")?.focus();
  }, []);
  useEffect(() => {
    if (!onClose) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose]);

  return (
    <div className="dialog-backdrop" onPointerDown={(e) => e.target === e.currentTarget && onClose?.()}>
      <div className={`dialog${wide ? " dialog-wide" : ""}`} role="dialog" aria-modal="true" aria-label={title} ref={ref}>
        <header className="dialog-head">
          {icon && (
            <span className={`dialog-icon tone-${tone}`}>
              <Icon name={icon} size={18} />
            </span>
          )}
          <h2>{title}</h2>
          {onClose && (
            <button type="button" className="btn btn-ghost btn-icon btn-sm dialog-close" onClick={onClose} aria-label="閉じる">
              <Icon name="close" size={14} />
            </button>
          )}
        </header>
        <div className="dialog-body">{children}</div>
        <footer className="dialog-actions">{actions}</footer>
      </div>
    </div>
  );
}
