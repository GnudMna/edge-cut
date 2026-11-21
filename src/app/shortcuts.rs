//! キーボードショートカット
//!
//! 1 フレーム分の入力から実行すべき操作を判定する
//! egui が Ctrl+C / Ctrl+V を `Key` イベントとして渡さない制約もここで吸収する

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};

/// 画像を開く
pub(super) const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);

/// 一括トリミング
pub(super) const BATCH_TRIM: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::O);

/// クリップボードから貼り付け (表示用。判定は `ShortcutTracker` で行う)
pub(super) const PASTE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::V);

/// トリミング後の画像を保存
pub(super) const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);

/// トリミング後の画像をコピー (表示用。判定は `Event::Copy` で行う)
pub(super) const COPY: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::C);

/// 画像をクリア
pub(super) const CLEAR: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::Delete);

/// スポイト解除・一括トリミングの中止
pub(super) const CANCEL: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::Escape);

/// ショートカットで実行する操作
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Shortcut {
    /// 画像ファイルを開く
    Open,
    /// 一括トリミングを開始する
    BatchTrim,
    /// クリップボードの画像を貼り付ける
    Paste,
    /// トリミング後の画像を保存する
    Save,
    /// トリミング後の画像をクリップボードへコピーする
    Copy,
    /// 画像をクリアする
    Clear,
    /// スポイトを解除する。スポイト中でなければ一括トリミングを中止する
    Cancel,
}

/// フレームをまたいでショートカットの判定に必要なキー状態を保持する
///
/// egui-winit は Ctrl+V の押下を `Key` イベントとして渡さず、
/// クリップボードが画像のときは `Event::Paste` も発生しない
/// 一方で離したときの `Key` イベントは届くため、押下が記録されないまま V が離されたことを
/// Ctrl+V の検出に使う
#[derive(Default)]
pub(super) struct ShortcutTracker {
    /// 前フレーム終了時点で V の押下が egui に記録されていたか
    v_down: bool,
    /// 直近の V を離すまでの間に Ctrl (macOS は Cmd) が押されていたか
    command_seen: bool,
}

impl ShortcutTracker {
    /// 入力から実行すべき操作を判定する
    ///
    /// `enabled` が `false` のときもキー状態の追跡は続け、操作は返さない
    /// 該当するキーは `input` から消費する (`Escape` はポップアップを閉じる用途と共存させるため消費しない)
    ///
    /// # Arguments
    /// * `input` - 現在フレームの入力状態
    /// * `enabled` - ショートカットを受け付けるか
    ///
    /// # Returns
    /// * `Option<Shortcut>` - 実行すべき操作。該当しなければ `None`
    pub(super) fn detect(
        &mut self,
        input: &mut egui::InputState,
        enabled: bool,
    ) -> Option<Shortcut> {
        let paste = self.track_paste(input);
        if !enabled {
            return None;
        }
        let copy = input.events.iter().any(|e| matches!(e, egui::Event::Copy));

        // Ctrl+O は Shift 付きにも一致するため、Ctrl+Shift+O を先に判定する
        if input.consume_shortcut(&BATCH_TRIM) {
            Some(Shortcut::BatchTrim)
        } else if input.consume_shortcut(&OPEN) {
            Some(Shortcut::Open)
        } else if input.consume_shortcut(&SAVE) {
            Some(Shortcut::Save)
        } else if paste {
            Some(Shortcut::Paste)
        } else if copy {
            Some(Shortcut::Copy)
        } else if input.consume_shortcut(&CLEAR) {
            Some(Shortcut::Clear)
        } else if input.key_pressed(CANCEL.logical_key) && input.modifiers.is_none() {
            Some(Shortcut::Cancel)
        } else {
            None
        }
    }

    /// 押下が記録されないまま Ctrl 押下中に V が離されたかを判定し、キー状態を更新する
    fn track_paste(&mut self, input: &egui::InputState) -> bool {
        if input.modifiers.command {
            self.command_seen = true;
        }
        let released = input.key_released(Key::V);
        // 押下と解放が同一フレームに届いた場合は通常の V 入力として扱う
        let pasted = released && self.command_seen && !self.v_down && !input.key_pressed(Key::V);
        if released {
            self.command_seen = false;
        }
        self.v_down = input.key_down(Key::V);
        pasted
    }
}

/// ホバーテキストの末尾にショートカット表記を付ける
///
/// # Arguments
/// * `ctx` - egui のコンテキスト (OS に合わせた表記に使用)
/// * `text` - 元のホバーテキスト
/// * `shortcut` - 付記するショートカット
///
/// # Returns
/// * `String` - `"{text} ({ショートカット})"` 形式の文字列
pub(super) fn with_shortcut(ctx: &egui::Context, text: &str, shortcut: KeyboardShortcut) -> String {
    format!("{text} ({})", ctx.format_shortcut(&shortcut))
}

// ======================================================================
// テスト
// ======================================================================
#[cfg(test)]
mod tests {
    use super::{Shortcut, ShortcutTracker};

    use eframe::egui::{self, Event, Key, Modifiers};

    // ======================================================================
    // テスト用のヘルパー関数
    // ======================================================================
    /// `Key` イベントを生成する
    fn key(key: Key, pressed: bool, modifiers: Modifiers) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        }
    }

    /// 1 フレーム分の入力を与えて判定結果を返す
    fn run_frame(
        ctx: &egui::Context,
        tracker: &mut ShortcutTracker,
        modifiers: Modifiers,
        events: Vec<Event>,
        enabled: bool,
    ) -> Option<Shortcut> {
        // 修飾キーの状態は `ModifiersChanged` イベントで渡す
        let raw = egui::RawInput {
            events: std::iter::once(Event::ModifiersChanged(modifiers))
                .chain(events)
                .collect(),
            ..Default::default()
        };
        let mut result = None;
        let mut output = ctx.run_ui(raw, |ui| {
            result = ui.ctx().input_mut(|i| tracker.detect(i, enabled));
        });
        // 未適用のテクスチャ差分を破棄しないと drop 時の debug_assert に引っかかる
        output.textures_delta.clear();
        result
    }

    // ======================================================================
    // 通常のキー
    // ======================================================================
    /// Ctrl+O で開く、Ctrl+Shift+O で一括トリミングと判定することを確認する
    #[test]
    fn detects_open_and_batch_trim() {
        let ctx = egui::Context::default();
        let mut tracker = ShortcutTracker::default();
        let cmd = Modifiers::COMMAND;
        let cmd_shift = Modifiers::COMMAND.plus(Modifiers::SHIFT);

        let r = run_frame(&ctx, &mut tracker, cmd, vec![key(Key::O, true, cmd)], true);
        assert_eq!(r, Some(Shortcut::Open));

        // Shift 付きは一括トリミングを優先する
        let events = vec![key(Key::O, true, cmd_shift)];
        let r = run_frame(&ctx, &mut tracker, cmd_shift, events, true);
        assert_eq!(r, Some(Shortcut::BatchTrim));
    }

    /// Ctrl+S・Delete・Escape をそれぞれ判定することを確認する
    #[test]
    fn detects_save_clear_cancel() {
        let ctx = egui::Context::default();
        let mut tracker = ShortcutTracker::default();
        let cmd = Modifiers::COMMAND;
        let none = Modifiers::NONE;

        let r = run_frame(&ctx, &mut tracker, cmd, vec![key(Key::S, true, cmd)], true);
        assert_eq!(r, Some(Shortcut::Save));
        let r = run_frame(
            &ctx,
            &mut tracker,
            none,
            vec![key(Key::Delete, true, none)],
            true,
        );
        assert_eq!(r, Some(Shortcut::Clear));
        let r = run_frame(
            &ctx,
            &mut tracker,
            none,
            vec![key(Key::Escape, true, none)],
            true,
        );
        assert_eq!(r, Some(Shortcut::Cancel));
    }

    /// 修飾キーなしの O・S は操作として判定しないことを確認する
    #[test]
    fn ignores_keys_without_command() {
        let ctx = egui::Context::default();
        let mut tracker = ShortcutTracker::default();
        let none = Modifiers::NONE;

        let events = vec![key(Key::O, true, none), key(Key::S, true, none)];
        assert_eq!(run_frame(&ctx, &mut tracker, none, events, true), None);
    }

    /// `Event::Copy` をコピーと判定することを確認する
    #[test]
    fn detects_copy_event() {
        let ctx = egui::Context::default();
        let mut tracker = ShortcutTracker::default();

        let r = run_frame(
            &ctx,
            &mut tracker,
            Modifiers::COMMAND,
            vec![Event::Copy],
            true,
        );
        assert_eq!(r, Some(Shortcut::Copy));
    }

    /// 無効時は操作を返さないことを確認する
    #[test]
    fn returns_none_when_disabled() {
        let ctx = egui::Context::default();
        let mut tracker = ShortcutTracker::default();
        let cmd = Modifiers::COMMAND;

        let r = run_frame(&ctx, &mut tracker, cmd, vec![key(Key::O, true, cmd)], false);
        assert_eq!(r, None);
    }

    // ======================================================================
    // 貼り付け
    // ======================================================================
    /// 押下が記録されないまま Ctrl 押下中に V が離されると貼り付けと判定することを確認する
    #[test]
    fn detects_paste_from_swallowed_press() {
        let ctx = egui::Context::default();
        let mut tracker = ShortcutTracker::default();
        let cmd = Modifiers::COMMAND;

        // egui-winit が V の押下を握りつぶしたフレーム
        assert_eq!(run_frame(&ctx, &mut tracker, cmd, vec![], true), None);
        let r = run_frame(&ctx, &mut tracker, cmd, vec![key(Key::V, false, cmd)], true);
        assert_eq!(r, Some(Shortcut::Paste));
    }

    /// Ctrl を先に離してから V を離しても貼り付けと判定することを確認する
    #[test]
    fn detects_paste_when_command_released_first() {
        let ctx = egui::Context::default();
        let mut tracker = ShortcutTracker::default();
        let none = Modifiers::NONE;

        run_frame(&ctx, &mut tracker, Modifiers::COMMAND, vec![], true);
        run_frame(&ctx, &mut tracker, none, vec![], true);
        let r = run_frame(
            &ctx,
            &mut tracker,
            none,
            vec![key(Key::V, false, none)],
            true,
        );
        assert_eq!(r, Some(Shortcut::Paste));
    }

    /// 通常の V の押下・解放は貼り付けと判定しないことを確認する
    #[test]
    fn ignores_plain_v() {
        let ctx = egui::Context::default();
        let mut tracker = ShortcutTracker::default();
        let none = Modifiers::NONE;

        // 押下と解放が別フレーム
        run_frame(
            &ctx,
            &mut tracker,
            none,
            vec![key(Key::V, true, none)],
            true,
        );
        let r = run_frame(
            &ctx,
            &mut tracker,
            none,
            vec![key(Key::V, false, none)],
            true,
        );
        assert_eq!(r, None);

        // Ctrl を押した後でも、押下と解放が同一フレームなら通常の入力として扱う
        run_frame(&ctx, &mut tracker, Modifiers::COMMAND, vec![], true);
        let events = vec![key(Key::V, true, none), key(Key::V, false, none)];
        assert_eq!(run_frame(&ctx, &mut tracker, none, events, true), None);
    }

    /// Ctrl が押されていなければ、押下の記録がない V の解放も貼り付けと判定しないことを確認する
    #[test]
    fn ignores_release_without_command() {
        let ctx = egui::Context::default();
        let mut tracker = ShortcutTracker::default();
        let none = Modifiers::NONE;

        let r = run_frame(
            &ctx,
            &mut tracker,
            none,
            vec![key(Key::V, false, none)],
            true,
        );
        assert_eq!(r, None);
    }

    /// 貼り付けの判定後は Ctrl の押下履歴をリセットすることを確認する
    #[test]
    fn resets_command_history_after_paste() {
        let ctx = egui::Context::default();
        let mut tracker = ShortcutTracker::default();
        let none = Modifiers::NONE;

        run_frame(&ctx, &mut tracker, Modifiers::COMMAND, vec![], true);
        run_frame(
            &ctx,
            &mut tracker,
            none,
            vec![key(Key::V, false, none)],
            true,
        );
        let r = run_frame(
            &ctx,
            &mut tracker,
            none,
            vec![key(Key::V, false, none)],
            true,
        );
        assert_eq!(r, None);
    }

    /// 無効時も V の状態を追跡し、有効に戻った後の誤判定を防ぐことを確認する
    #[test]
    fn tracks_state_while_disabled() {
        let ctx = egui::Context::default();
        let mut tracker = ShortcutTracker::default();
        let cmd = Modifiers::COMMAND;

        let r = run_frame(
            &ctx,
            &mut tracker,
            cmd,
            vec![key(Key::V, false, cmd)],
            false,
        );
        assert_eq!(r, None);
        // 無効時の解放で履歴が消費されていれば、通常の V の解放は貼り付けにならない
        let none = Modifiers::NONE;
        let r = run_frame(
            &ctx,
            &mut tracker,
            none,
            vec![key(Key::V, false, none)],
            true,
        );
        assert_eq!(r, None);
    }
}
