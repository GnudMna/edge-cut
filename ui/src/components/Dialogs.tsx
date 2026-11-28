// 更新・一括トリミングのエラー・終了確認のダイアログ

import { send } from "../bridge";
import { formatBytes } from "../lib/format";
import type { Snapshot } from "../types";
import { Button } from "./controls";
import { Dialog } from "./overlays";

export function Dialogs({ s }: { s: Snapshot }) {
  return (
    <>
      {s.update.open && <UpdateDialog s={s} />}
      {s.batch.errorsOpen && s.batch.errors.length > 0 && <BatchErrorsDialog errors={s.batch.errors} />}
      {s.closeConfirm && <CloseConfirmDialog />}
    </>
  );
}

function UpdateDialog({ s }: { s: Snapshot }) {
  const phase = s.update.phase;
  const close = () => send({ type: "closeUpdate" });
  const closeButton = (
    <Button onClick={close} key="close">
      閉じる
    </Button>
  );

  switch (phase.kind) {
    case "idle":
    case "checking":
      return (
        <Dialog title="アップデート" icon="refresh" onClose={close} actions={closeButton}>
          <div className="dialog-progress">
            <span className="spinner spinner-sm" />
            更新を確認しています…
          </div>
        </Dialog>
      );
    case "upToDate":
      return (
        <Dialog
          title="最新版を使用しています"
          icon="check"
          tone="success"
          onClose={close}
          actions={
            <>
              <Button onClick={() => send({ type: "checkUpdate" })}>再確認</Button>
              <Button variant="primary" onClick={close}>
                閉じる
              </Button>
            </>
          }
        >
          <p>現在のバージョン: v{phase.current}</p>
        </Dialog>
      );
    case "available":
      return (
        <Dialog
          title="新しいバージョンがあります"
          icon="download"
          onClose={close}
          actions={
            <>
              <Button onClick={close}>後で</Button>
              <Button variant="primary" icon="download" disabled={s.update.applyBlocked} onClick={() => send({ type: "applyUpdate" })}>
                更新する
              </Button>
            </>
          }
        >
          <dl className="kv">
            <dt>現在</dt>
            <dd>v{phase.current}</dd>
            <dt>最新</dt>
            <dd>
              <strong>v{phase.latest}</strong>
            </dd>
            <dt>サイズ</dt>
            <dd>{formatBytes(phase.size)}</dd>
          </dl>
          {s.update.applyBlocked ? (
            <p className="note note-warn">一括トリミングまたは保存の完了後に更新できる</p>
          ) : (
            <p className="muted small">更新プログラムがダウンロードと検証を行い、EdgeCut を終了して更新した後に起動し直す。ダウンロード中も EdgeCut は使える</p>
          )}
        </Dialog>
      );
    case "error":
      return (
        <Dialog
          title="更新に失敗しました"
          icon="alert"
          tone="danger"
          onClose={close}
          actions={
            <>
              <Button onClick={close}>閉じる</Button>
              <Button variant="primary" onClick={() => send({ type: "checkUpdate" })}>
                再試行
              </Button>
            </>
          }
        >
          <p className="error-text">{phase.message}</p>
        </Dialog>
      );
    case "updaterRunning":
      return (
        <Dialog title="アップデート" icon="download" onClose={close} actions={closeButton}>
          <p>更新プログラムで v{phase.latest} への更新を進めている</p>
          <p className="muted small">更新プログラムの画面を閉じると、更新せずに戻る</p>
        </Dialog>
      );
    case "exiting":
      return (
        <Dialog title="アップデート" icon="refresh" actions={null}>
          <div className="dialog-progress">
            <span className="spinner spinner-sm" />
            更新のため EdgeCut を終了しています…
          </div>
          <p className="muted small">保存中の画像があれば、完了してから終了する</p>
        </Dialog>
      );
  }
}

function BatchErrorsDialog({ errors }: { errors: string[] }) {
  const close = () => send({ type: "closeBatchErrors" });
  return (
    <Dialog
      title="一括トリミングのエラー"
      icon="alert"
      tone="danger"
      wide
      onClose={close}
      actions={
        <>
          <Button icon="copy" onClick={() => void navigator.clipboard?.writeText(errors.join("\n"))}>
            すべてコピー
          </Button>
          <Button variant="primary" onClick={close}>
            閉じる
          </Button>
        </>
      }
    >
      <p>{errors.length} 件のファイルが失敗した</p>
      <ul className="error-list">
        {errors.map((e, i) => (
          <li key={i} className="mono">
            {e}
          </li>
        ))}
      </ul>
    </Dialog>
  );
}

function CloseConfirmDialog() {
  return (
    <Dialog
      title="一括トリミングを中止して終了しますか？"
      icon="alert"
      tone="danger"
      onClose={() => send({ type: "cancelClose" })}
      actions={
        <>
          <Button onClick={() => send({ type: "cancelClose" })}>続ける</Button>
          <Button variant="danger" onClick={() => send({ type: "confirmClose" })}>
            中止して終了
          </Button>
        </>
      }
    >
      <p className="muted">処理中のファイルは完了させ、未着手のファイルは処理しない</p>
    </Dialog>
  );
}
