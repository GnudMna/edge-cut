// 設定パネルなどで使う入力部品

import { type ReactNode, useEffect, useId, useRef, useState } from "react";

import { Icon, type IconName } from "./Icon";

// ======================================================================
// ボタン
// ======================================================================
interface ButtonProps {
  icon?: IconName;
  children?: ReactNode;
  onClick?: () => void;
  disabled?: boolean;
  variant?: "primary" | "default" | "ghost" | "danger";
  title?: string;
  size?: "sm" | "md";
  active?: boolean;
  className?: string;
}

export function Button({
  icon,
  children,
  onClick,
  disabled,
  variant = "default",
  title,
  size = "md",
  active,
  className,
}: ButtonProps) {
  const classes = ["btn", `btn-${variant}`, `btn-${size}`];
  if (active) classes.push("is-active");
  if (!children) classes.push("btn-icon");
  if (className) classes.push(className);
  return (
    <button
      type="button"
      className={classes.join(" ")}
      onClick={onClick}
      disabled={disabled}
      title={title}
      aria-label={children ? undefined : title}
      aria-pressed={active}
    >
      {icon && <Icon name={icon} />}
      {children && <span>{children}</span>}
    </button>
  );
}

// ======================================================================
// 数値
// ======================================================================
interface NumberFieldProps {
  value: number;
  min: number;
  max: number;
  onCommit: (value: number) => void;
  disabled?: boolean;
  label?: string;
  unit?: string;
  width?: number;
}

/** 数値の直接入力。Enter・フォーカスを外したときに範囲内へ収めて確定する */
export function NumberField({ value, min, max, onCommit, disabled, label, unit, width = 56 }: NumberFieldProps) {
  const [text, setText] = useState(String(value));
  const focused = useRef(false);

  useEffect(() => {
    if (!focused.current) setText(String(value));
  }, [value]);

  const commit = (raw: string) => {
    const parsed = Number.parseInt(raw, 10);
    const next = Number.isNaN(parsed) ? value : Math.min(max, Math.max(min, parsed));
    setText(String(next));
    if (next !== value) onCommit(next);
  };

  return (
    <label className={`number-field${disabled ? " is-disabled" : ""}`} style={{ width }}>
      <input
        type="text"
        inputMode="numeric"
        aria-label={label}
        value={text}
        disabled={disabled}
        onFocus={(e) => {
          focused.current = true;
          e.currentTarget.select();
        }}
        onBlur={(e) => {
          focused.current = false;
          commit(e.currentTarget.value);
        }}
        onChange={(e) => setText(e.currentTarget.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            commit(e.currentTarget.value);
            e.currentTarget.blur();
          } else if (e.key === "Escape") {
            setText(String(value));
            e.currentTarget.blur();
          } else if (e.key === "ArrowUp" || e.key === "ArrowDown") {
            e.preventDefault();
            const step = (e.shiftKey ? 10 : 1) * (e.key === "ArrowUp" ? 1 : -1);
            const next = Math.min(max, Math.max(min, value + step));
            setText(String(next));
            if (next !== value) onCommit(next);
          }
        }}
      />
      {unit && <span className="number-unit">{unit}</span>}
    </label>
  );
}

// ======================================================================
// スライダー
// ======================================================================
interface SliderProps {
  label: string;
  value: number;
  min: number;
  max: number;
  unit?: string;
  hint?: string;
  disabled?: boolean;
  /** 値の変更。`live` はドラッグ中 (続けて値が変わる途中) */
  onChange: (value: number, live: boolean) => void;
  /** ラベルの右に置く部品 */
  accessory?: ReactNode;
}

/** ラベル・スライダー・数値入力の組 */
export function Slider({ label, value, min, max, unit, hint, disabled, onChange, accessory }: SliderProps) {
  const id = useId();
  const [draft, setDraft] = useState<number | null>(null);
  const dragging = useRef(false);
  const shown = draft ?? value;
  const percent = max > min ? ((shown - min) / (max - min)) * 100 : 0;

  const finish = () => {
    if (!dragging.current) return;
    dragging.current = false;
    onChange(shown, false);
    setDraft(null);
  };

  return (
    <div className={`field slider-field${disabled ? " is-disabled" : ""}`}>
      <div className="field-head">
        <label htmlFor={id} title={hint}>
          {label}
        </label>
        {accessory}
      </div>
      <div className="slider-row">
        <input
          id={id}
          type="range"
          className="range"
          min={min}
          max={max}
          step={1}
          value={shown}
          disabled={disabled}
          title={hint}
          style={{ "--fill": `${percent}%` } as React.CSSProperties}
          onPointerDown={() => {
            dragging.current = true;
          }}
          onPointerUp={finish}
          onLostPointerCapture={finish}
          onChange={(e) => {
            const v = Number(e.currentTarget.value);
            if (dragging.current) setDraft(v);
            onChange(v, dragging.current);
          }}
        />
        <NumberField
          value={shown}
          min={min}
          max={max}
          unit={unit}
          label={label}
          disabled={disabled}
          onCommit={(v) => onChange(v, false)}
        />
      </div>
    </div>
  );
}

// ======================================================================
// スイッチ・選択
// ======================================================================
interface SwitchProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: ReactNode;
  hint?: string;
  disabled?: boolean;
}

export function Switch({ checked, onChange, label, hint, disabled }: SwitchProps) {
  return (
    <label className={`switch${disabled ? " is-disabled" : ""}`} title={hint}>
      <input
        type="checkbox"
        role="switch"
        checked={checked}
        disabled={disabled}
        onChange={(e) => onChange(e.currentTarget.checked)}
      />
      <span className="switch-track" aria-hidden="true">
        <span className="switch-thumb" />
      </span>
      <span className="switch-label">{label}</span>
    </label>
  );
}

interface SegmentedProps<T extends string> {
  value: T;
  options: { value: T; label: ReactNode; title?: string }[];
  onChange: (value: T) => void;
  label: string;
  size?: "sm" | "md";
  className?: string;
}

export function Segmented<T extends string>({ value, options, onChange, label, size = "md", className }: SegmentedProps<T>) {
  return (
    <div className={`segmented segmented-${size} ${className ?? ""}`} role="radiogroup" aria-label={label}>
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          role="radio"
          aria-checked={o.value === value}
          className={o.value === value ? "is-selected" : ""}
          title={o.title}
          onClick={() => onChange(o.value)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

// ======================================================================
// 折りたたみ
// ======================================================================
interface SectionProps {
  title: string;
  summary?: string;
  children: ReactNode;
  collapsible?: boolean;
  defaultOpen?: boolean;
  storageKey?: string;
  accessory?: ReactNode;
}

/** 設定パネルの区切り。開閉状態は `localStorage` に覚える (失敗しても既定値で動く) */
export function Section({ title, summary, children, collapsible, defaultOpen = true, storageKey, accessory }: SectionProps) {
  const [open, setOpen] = useState(() => {
    if (!collapsible || !storageKey) return true;
    try {
      const saved = localStorage.getItem(storageKey);
      return saved === null ? defaultOpen : saved === "1";
    } catch {
      return defaultOpen;
    }
  });

  const toggle = () => {
    const next = !open;
    setOpen(next);
    if (storageKey) {
      try {
        localStorage.setItem(storageKey, next ? "1" : "0");
      } catch {
        // 保存できなくても開閉はできる
      }
    }
  };

  return (
    <section className={`section${open ? " is-open" : ""}`}>
      <header className="section-head">
        {collapsible ? (
          <button type="button" className="section-toggle" onClick={toggle} aria-expanded={open}>
            <Icon name="chevron" className="section-chevron" size={14} />
            <span className="section-title">{title}</span>
            {!open && summary && <span className="section-summary">{summary}</span>}
          </button>
        ) : (
          <span className="section-title">{title}</span>
        )}
        {accessory}
      </header>
      {open && <div className="section-body">{children}</div>}
    </section>
  );
}
