// 設定パネル (トリミング結果・トリミング・背景色・詳細設定・出力)

import { useEffect, useRef, useState } from "react";

import { send } from "../bridge";
import {
  ASPECT_PRESETS,
  FORMAT_LABELS,
  OUTPUT_FORMATS,
  areaChangePercent,
  cssColor,
  formatChange,
  outputSummary,
  parseHex,
  rgbHex,
  trimDetailSummary,
} from "../lib/format";
import type { Prefs, Rgba, Sides, Snapshot, TrimPatch } from "../types";
import { Button, NumberField, Section, Segmented, Slider, Switch } from "./controls";
import { Icon } from "./Icon";
import { Popover } from "./overlays";

const setTrim = (patch: TrimPatch, live = false) => send({ type: "setTrim", patch, live });

const SIDE_NAMES: Record<keyof Sides<unknown>, string> = { top: "上", right: "右", bottom: "下", left: "左" };
const SIDE_KEYS = ["top", "right", "bottom", "left"] as const;

export function Inspector({ s }: { s: Snapshot }) {
  const p = s.prefs;
  return (
    <aside className="inspector" style={{ width: p.panelWidth }}>
      <PanelResizer s={s} />
      <ResultCard s={s} />
      <div className="inspector-scroll">
        <Section title="トリミング">
          <Slider
            label="許容度"
            value={p.tolerance}
            min={s.limits.tolerance[0]}
            max={s.limits.tolerance[1]}
            hint="背景色との類似度。値が大きいほど広い範囲を余白とみなす"
            onChange={(v, live) => setTrim({ tolerance: v }, live)}
          />
          <MarginControls s={s} />
          <BackgroundControls s={s} />
        </Section>

        <Section title="詳細設定" collapsible defaultOpen={false} storageKey="section.details" summary={trimDetailSummary(p)}>
          <Slider
            label="ノイズ除外"
            value={p.ignorePixels}
            min={s.limits.ignorePixels[0]}
            max={s.limits.ignorePixels[1]}
            unit="px"
            hint="行・列あたりこの画素数以下の小さなゴミ (スキャンの埃や JPEG ノイズ) を内容とみなさない。0 のとき 1 画素でも内容とする"
            onChange={(v, live) => setTrim({ ignorePixels: v }, live)}
          />
          <SideControls p={p} />
          <AspectControls s={s} />
        </Section>

        <Section title="出力" collapsible defaultOpen storageKey="section.output" summary={outputSummary(p)}>
          <OutputControls s={s} />
        </Section>
      </div>
    </aside>
  );
}

// ======================================================================
// パネル幅
// ======================================================================
function PanelResizer({ s }: { s: Snapshot }) {
  const [min, max] = s.limits.panelWidth;
  const [draft, setDraft] = useState<number | null>(null);
  const start = useRef<{ x: number; w: number } | null>(null);

  return (
    <div
      className="panel-resizer"
      role="separator"
      aria-orientation="vertical"
      title="ドラッグで幅を変える (ダブルクリックで既定の幅)"
      onPointerDown={(e) => {
        e.currentTarget.setPointerCapture(e.pointerId);
        start.current = { x: e.clientX, w: s.prefs.panelWidth };
      }}
      onPointerMove={(e) => {
        if (!start.current) return;
        const w = Math.round(Math.min(max, Math.max(min, start.current.w + start.current.x - e.clientX)));
        setDraft(w);
        const aside = e.currentTarget.parentElement;
        if (aside) aside.style.width = `${w}px`;
      }}
      onPointerUp={() => {
        if (draft !== null) send({ type: "setPanel", width: draft });
        start.current = null;
        setDraft(null);
      }}
      onDoubleClick={() => send({ type: "setPanel", width: 340 })}
    />
  );
}

// ======================================================================
// トリミング結果
// ======================================================================
function ResultCard({ s }: { s: Snapshot }) {
  const image = s.image;
  const trim = image?.trim;
  const canOutput = s.busy.canOutput && !s.busy.dialog;
  const change = image && trim ? areaChangePercent([image.width, image.height], [trim.rect.w, trim.rect.h]) : null;

  return (
    <div className="result-card">
      {!image ? (
        <p className="muted result-empty">画像を開くとトリミング結果を表示する</p>
      ) : (
        <>
          <div className="result-head">
            <span className="result-label">トリミング後</span>
            {(s.busy.trimming || s.busy.saving) && (
              <span className="result-busy">
                <span className="spinner spinner-sm" />
                {s.busy.saving ? "保存中" : "計算中"}
              </span>
            )}
          </div>
          <div className={`result-size${s.busy.trimming ? " is-stale" : ""}`}>
            {trim ? (
              <>
                {trim.rect.w}
                <span className="times">×</span>
                {trim.rect.h}
                <span className="unit">px</span>
              </>
            ) : (
              <span className="muted">—</span>
            )}
          </div>
          <div className="result-meta">
            {change !== null && (
              <span className={`chip ${change < 0 ? "chip-good" : change > 0 ? "chip-warn" : ""}`} title="元画像に対する面積の増減">
                面積 {formatChange(change)}
              </span>
            )}
            <span className="chip" title="元画像のサイズ">
              元 {image.width} × {image.height}
            </span>
            {trim && (
              <span className="chip" title="トリミング範囲の左上の位置 (元画像の座標)">
                x {trim.rect.x} · y {trim.rect.y}
              </span>
            )}
          </div>
        </>
      )}
      <div className="result-actions">
        <Button
          icon="save"
          variant="primary"
          disabled={!canOutput}
          onClick={() => send({ type: "save" })}
          title="トリミング後の画像を保存 (Ctrl+S)"
          className="grow"
        >
          保存{image ? ` (.${image.outputExt})` : ""}
        </Button>
        <Button icon="copy" disabled={!canOutput} onClick={() => send({ type: "copy" })} title="トリミング後の画像をクリップボードへコピー (Ctrl+C)">
          コピー
        </Button>
      </div>
    </div>
  );
}

// ======================================================================
// 余白
// ======================================================================
function MarginControls({ s }: { s: Snapshot }) {
  const p = s.prefs;
  const [min, max] = s.limits.padding;
  const link = (
    <Button
      icon={p.linkMargins ? "link" : "unlink"}
      variant="ghost"
      size="sm"
      active={p.linkMargins}
      onClick={() => setTrim({ linkMargins: !p.linkMargins })}
      title={p.linkMargins ? "四辺を連動中。押すと辺ごとに指定できる" : "辺ごとに指定中。押すと四辺を連動させる"}
    />
  );

  if (p.linkMargins) {
    return (
      <Slider
        label="余白"
        value={p.padding}
        min={min}
        max={max}
        unit="px"
        hint="トリミング後に残す余白ピクセル数。画像の端を越える分は背景色で塗って追加する"
        onChange={(v, live) => setTrim({ padding: v }, live)}
        accessory={link}
      />
    );
  }

  const field = (side: (typeof SIDE_KEYS)[number]) => (
    <NumberField
      value={p.margins[side]}
      min={min}
      max={max}
      label={`${SIDE_NAMES[side]}の余白`}
      disabled={!p.sides[side]}
      onCommit={(v) => setTrim({ margins: { ...p.margins, [side]: v } })}
    />
  );

  return (
    <div className="field">
      <div className="field-head">
        <span title="辺ごとに残す余白ピクセル数 (除去しない辺には余白を加えない)">余白 (辺ごと)</span>
        {link}
      </div>
      <div className="margin-box">
        <div className="mb-top">{field("top")}</div>
        <div className="mb-left">{field("left")}</div>
        <div className="mb-center" aria-hidden="true">
          px
        </div>
        <div className="mb-right">{field("right")}</div>
        <div className="mb-bottom">{field("bottom")}</div>
      </div>
    </div>
  );
}

// ======================================================================
// 背景色
// ======================================================================
function BackgroundControls({ s }: { s: Snapshot }) {
  const bg = s.background;
  const hasImage = s.image !== null;
  const [open, setOpen] = useState(false);

  return (
    <div className="field">
      <div className="field-head">
        <span>背景色</span>
        {bg.color && <span className={`badge ${bg.manual ? "badge-warn" : ""}`}>{bg.manual ? "手動指定" : "自動検出"}</span>}
      </div>
      {!hasImage && !bg.manual ? (
        <p className="muted small">画像を開くと四辺から自動検出する</p>
      ) : (
        <div className="bg-row">
          <Popover
            open={open}
            onClose={() => setOpen(false)}
            anchor={
              <button type="button" className="bg-swatch" onClick={() => setOpen((v) => !v)} title="クリックして背景色を直接編集" disabled={!bg.color}>
                <span className="checker-sm">
                  <span style={{ background: bg.color ? cssColor(bg.color) : "transparent" }} />
                </span>
                <span className="mono">{bg.color ? `${rgbHex(bg.color)} · α ${bg.color[3]}` : "—"}</span>
              </button>
            }
          >
            {bg.color && <ColorEditor color={bg.color} />}
          </Popover>
          <div className="bg-actions">
            <Button
              icon="eyedropper"
              size="sm"
              active={bg.picking}
              disabled={!hasImage}
              onClick={() => send({ type: "setPickMode", enabled: !bg.picking })}
              title="元画像をクリックして背景色を取得する (Esc で解除)"
            />
            <Button icon="reset" size="sm" disabled={!bg.manual} onClick={() => send({ type: "resetBackground" })} title="手動指定を解除し、四辺から背景色を自動検出する" />
          </div>
        </div>
      )}
    </div>
  );
}

/** 背景色の直接編集。続けて値を変えている間は送らず、止まってからまとめて送る */
function ColorEditor({ color }: { color: Rgba }) {
  const [draft, setDraft] = useState<Rgba>(color);
  const [hex, setHex] = useState(rgbHex(color));
  const timer = useRef<number | undefined>(undefined);

  const update = (next: Rgba) => {
    setDraft(next);
    setHex(rgbHex(next));
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => send({ type: "setBackground", rgba: next }), 150);
  };
  useEffect(() => () => window.clearTimeout(timer.current), []);

  return (
    <div className="color-editor">
      <div className="color-preview checker-sm">
        <span style={{ background: cssColor(draft) }} />
      </div>
      <label className="color-field">
        <span>色</span>
        <input type="color" value={rgbHex(draft)} onChange={(e) => {
          const c = parseHex(e.currentTarget.value, draft[3]);
          if (c) update(c);
        }} />
        <input
          type="text"
          className="mono"
          value={hex}
          spellCheck={false}
          aria-label="16 進数の色"
          onChange={(e) => {
            setHex(e.currentTarget.value);
            const c = parseHex(e.currentTarget.value, draft[3]);
            if (c) update(c);
          }}
        />
      </label>
      <div className="rgba-fields">
        {(["R", "G", "B", "A"] as const).map((name, i) => (
          <label key={name}>
            <span>{name}</span>
            <NumberField
              value={draft[i] ?? 0}
              min={0}
              max={255}
              width={52}
              label={name}
              onCommit={(v) => {
                const next = [...draft] as Rgba;
                next[i] = v;
                update(next);
              }}
            />
          </label>
        ))}
      </div>
    </div>
  );
}

// ======================================================================
// 詳細設定
// ======================================================================
function SideControls({ p }: { p: Prefs }) {
  const toggle = (side: (typeof SIDE_KEYS)[number]) => setTrim({ sides: { ...p.sides, [side]: !p.sides[side] } });
  return (
    <div className="field">
      <div className="field-head">
        <span title="外した辺は元画像の端を維持する (その辺の余白も加えない)">除去する辺</span>
      </div>
      <div className="side-picker">
        <div className="side-box" role="group" aria-label="除去する辺">
          {SIDE_KEYS.map((side) => (
            <button
              key={side}
              type="button"
              className={`side side-${side}${p.sides[side] ? " is-on" : ""}`}
              aria-pressed={p.sides[side]}
              title={`${SIDE_NAMES[side]}の余白を${p.sides[side] ? "除去する" : "維持する"}`}
              onClick={() => toggle(side)}
            />
          ))}
          <span className="side-content" aria-hidden="true" />
        </div>
        <div className="side-legend">
          {SIDE_KEYS.map((side) => (
            <label key={side} className="check">
              <input type="checkbox" checked={p.sides[side]} onChange={() => toggle(side)} />
              <span>{SIDE_NAMES[side]}</span>
            </label>
          ))}
        </div>
      </div>
    </div>
  );
}

function AspectControls({ s }: { s: Snapshot }) {
  const p = s.prefs;
  const [min, max] = s.limits.aspectPart;
  const preset = ASPECT_PRESETS.find(([w, h]) => w === p.aspectW && h === p.aspectH);
  return (
    <div className="field">
      <Switch
        checked={p.aspectEnabled}
        onChange={(v) => setTrim({ aspectEnabled: v })}
        label="アスペクト比を固定"
        hint="余白を加えた後の範囲を指定の比率 (幅:高さ) に合わせる"
      />
      {p.aspectEnabled && (
        <div className="aspect">
          <div className="chips" role="radiogroup" aria-label="プリセット">
            {ASPECT_PRESETS.map(([w, h]) => (
              <button
                key={`${w}:${h}`}
                type="button"
                role="radio"
                aria-checked={preset?.[0] === w && preset?.[1] === h}
                className={`chip-btn${preset?.[0] === w && preset?.[1] === h ? " is-selected" : ""}`}
                onClick={() => setTrim({ aspectW: w, aspectH: h })}
              >
                <span className="ratio-glyph" style={{ aspectRatio: `${w} / ${h}` }} />
                {w}:{h}
              </button>
            ))}
          </div>
          <div className="aspect-row">
            <NumberField value={p.aspectW} min={min} max={max} width={64} label="幅の比" onCommit={(v) => setTrim({ aspectW: v })} />
            <span className="muted">:</span>
            <NumberField value={p.aspectH} min={min} max={max} width={64} label="高さの比" onCommit={(v) => setTrim({ aspectH: v })} />
            <Button icon="swap" size="sm" variant="ghost" onClick={() => setTrim({ aspectW: p.aspectH, aspectH: p.aspectW })} title="幅と高さの比を入れ替える" />
            {!preset && <span className="muted small">カスタム</span>}
          </div>
          <Segmented
            label="合わせ方"
            value={p.aspectFit}
            onChange={(v) => setTrim({ aspectFit: v })}
            options={[
              { value: "Expand", label: "余白を足す", title: "内容を削らず、短い方の辺を背景色で塗って足す" },
              { value: "Crop", label: "切り詰める", title: "長い方の辺を中央から削る (内容が欠ける場合がある)" },
            ]}
          />
        </div>
      )}
    </div>
  );
}

// ======================================================================
// 出力
// ======================================================================
function OutputControls({ s }: { s: Snapshot }) {
  const p = s.prefs;
  const showJpeg = p.outputFormat === "Jpeg" || p.outputFormat === "Original";
  const showWebp = p.outputFormat === "Webp" || p.outputFormat === "Original";
  return (
    <>
      <div className="field">
        <div className="field-head">
          <span title="保存・自動保存・一括トリミングの出力形式。「元の形式」は元画像と同じ形式 (クリップボードの画像などは PNG)">形式</span>
        </div>
        <div className="format-grid" role="radiogroup" aria-label="出力形式">
          {OUTPUT_FORMATS.map((f) => (
            <button
              key={f}
              type="button"
              role="radio"
              aria-checked={p.outputFormat === f}
              className={`chip-btn${p.outputFormat === f ? " is-selected" : ""}${f === "Original" ? " span-2" : ""}`}
              onClick={() => send({ type: "setOutput", patch: { format: f } })}
            >
              {FORMAT_LABELS[f]}
            </button>
          ))}
        </div>
      </div>
      {showJpeg && (
        <Slider
          label="JPEG 品質"
          value={p.jpegQuality}
          min={s.limits.jpegQuality[0]}
          max={s.limits.jpegQuality[1]}
          hint="JPEG で保存するときの品質。大きいほど高画質でファイルサイズも大きくなる"
          onChange={(v, live) => !live && send({ type: "setOutput", patch: { jpegQuality: v } })}
        />
      )}
      {showWebp && (
        <Slider
          label="WebP 品質"
          value={p.webpQuality}
          min={s.limits.webpQuality[0]}
          max={s.limits.webpQuality[1]}
          hint="WebP で保存するときの品質。100 のときだけロスレスで保存する"
          onChange={(v, live) => !live && send({ type: "setOutput", patch: { webpQuality: v } })}
          accessory={p.webpQuality >= s.limits.webpQuality[1] ? <span className="badge">ロスレス</span> : undefined}
        />
      )}
      <div className="field">
        <Switch
          checked={p.overwriteOutput}
          onChange={(v) => send({ type: "setOutput", patch: { overwriteOutput: v } })}
          label="同名ファイルを上書き"
          hint="自動保存と一括トリミングで、同名のファイルがあるときに連番を付けず上書きする"
        />
      </div>
      <div className="field">
        <Switch
          checked={p.autoSave}
          onChange={(v) => send({ type: "setAutoSave", enabled: v })}
          label="自動保存"
          hint="画像を開いた直後にトリミング結果を保存先フォルダへ保存する"
        />
        <div className="dir-row">
          <Icon name="folder" size={14} />
          <span className="dir-path mono" title={p.autoSaveDir}>
            {p.autoSaveDir}
          </span>
          <Button size="sm" variant="ghost" onClick={() => send({ type: "pickAutoSaveDir" })} title="自動保存の保存先フォルダを変更">
            変更…
          </Button>
        </div>
      </div>
    </>
  );
}
