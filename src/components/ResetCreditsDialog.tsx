import { useEffect, useMemo, useState } from 'react';
import { createPortal } from 'react-dom';
import { m } from 'motion/react';
import { X } from '@phosphor-icons/react';
import {
  consumeCodexResetCredit,
  readCodexResetCredits,
  type CodexResetCredit,
  type CodexResetCredits,
  type ResetCreditOutcome,
} from '../lib/desktop';
import { expiryLabel } from '../lib/format';
import { useAppState } from '../lib/store';
import { useDialogA11y } from '../lib/useDialogA11y';
import styles from '../App.module.css';

interface ResetCreditsDialogProps {
  accountId: string;
  accountName: string;
  /** 打开弹窗的按钮，关闭后把焦点还给它。 */
  trigger: HTMLElement | null;
  onClose: () => void;
}

type Phase = 'list' | 'confirm';

interface OutcomeNotice {
  kind: 'success' | 'info' | 'error';
  text: string;
}

function creditTitle(credit: CodexResetCredit): string {
  return credit.title?.trim() || 'Codex 额度重置卡';
}

function creditExpiry(credit: CodexResetCredit): string {
  return credit.expiresAtEpoch === null
    ? '不过期'
    : expiryLabel(credit.expiresAtEpoch);
}

/** 优先选最早到期的一张，避免先浪费有效期更长的卡。 */
function earliestAvailable(credits: CodexResetCredit[]): CodexResetCredit | null {
  const available = credits.filter((credit) => credit.status === 'available');
  if (available.length === 0) return null;
  return [...available].sort((left, right) => {
    if (left.expiresAtEpoch === null) return 1;
    if (right.expiresAtEpoch === null) return -1;
    return left.expiresAtEpoch - right.expiresAtEpoch;
  })[0];
}

function newIdempotencyKey(): string {
  if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) {
    return crypto.randomUUID();
  }
  return `tr-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function outcomeNotice(outcome: ResetCreditOutcome): OutcomeNotice {
  switch (outcome) {
    case 'reset':
      return { kind: 'success', text: '已使用一张重置卡，额度正在刷新。' };
    case 'nothingToReset':
      return {
        kind: 'info',
        text: '当前用量还不需要重置，这次没有消耗重置卡。',
      };
    case 'noCredit':
      return { kind: 'info', text: '账户没有可用的重置卡。' };
    case 'alreadyRedeemed':
      return {
        kind: 'info',
        text: '这次重置已经完成过，没有重复消耗；请查看刷新后的额度。',
      };
  }
}

/**
 * Codex 重置卡面板：明细按需读取，消耗必须经过二次确认。
 * 操作不可撤销；同一次确认内重试复用同一个幂等标识，避免重复扣卡。
 */
export default function ResetCreditsDialog({
  accountId,
  accountName,
  trigger,
  onClose,
}: ResetCreditsDialogProps) {
  const { desktop } = useAppState();
  const [details, setDetails] = useState<CodexResetCredits | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [phase, setPhase] = useState<Phase>('list');
  const [idempotencyKey, setIdempotencyKey] = useState('');
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<OutcomeNotice | null>(null);

  // 消耗请求进行中不允许 Esc 关闭：结果可能已经产生，关闭会丢掉提示。
  const requestClose = () => {
    if (!busy) onClose();
  };

  const { dialogRef, captureTrigger } = useDialogA11y(
    true,
    `${phase}:${loading}:${notice?.kind ?? ''}`,
    requestClose,
  );

  useEffect(() => {
    captureTrigger(trigger);
    // 弹窗组件会随关闭直接卸载，焦点还原不能依赖 useDialogA11y 的 open 切换。
    return () => {
      if (trigger?.isConnected) trigger.focus();
    };
  }, [captureTrigger, trigger]);

  useEffect(() => {
    if (!desktop) {
      setLoading(false);
      setLoadError('浏览器预览无法读取重置卡。');
      return;
    }
    let disposed = false;
    setLoading(true);
    setLoadError(null);
    void readCodexResetCredits(accountId).then(
      (result) => {
        if (!disposed) {
          setDetails(result);
          setLoading(false);
        }
      },
      (error: unknown) => {
        if (!disposed) {
          setLoadError(
            error instanceof Error ? error.message : String(error),
          );
          setLoading(false);
        }
      },
    );
    return () => {
      disposed = true;
    };
  }, [accountId, desktop]);

  const availableCredits = useMemo(
    () => (details?.credits ?? []).filter((credit) => credit.status === 'available'),
    [details],
  );
  const selected = useMemo(
    () => earliestAvailable(details?.credits ?? []),
    [details],
  );
  const availableCount = details?.availableCount ?? 0;

  function beginConfirm() {
    // 每次进入确认页都视为一次新的逻辑操作；确认页内重试沿用同一个 key。
    setIdempotencyKey(newIdempotencyKey());
    setNotice(null);
    setPhase('confirm');
  }

  function cancelConfirm() {
    setPhase('list');
    setIdempotencyKey('');
    setNotice(null);
  }

  function useCredit() {
    if (busy || !idempotencyKey) return;
    setBusy(true);
    setNotice(null);
    void consumeCodexResetCredit(accountId, selected?.id ?? null, idempotencyKey).then(
      (outcome) => {
        setBusy(false);
        setNotice(outcomeNotice(outcome));
        if (outcome === 'reset') {
          // 后端已扣卡并触发刷新；本地数量先减一，避免关闭前仍显示旧值。
          setDetails((current) =>
            current
              ? {
                  availableCount: Math.max(0, current.availableCount - 1),
                  credits:
                    current.credits?.filter((credit) => credit.id !== selected?.id) ?? null,
                }
              : current,
          );
        }
      },
      (error: unknown) => {
        setBusy(false);
        // 超时等未知结果可能已经扣卡；提示先确认额度，重试仍复用同一幂等标识。
        setNotice({
          kind: 'error',
          text: `${error instanceof Error ? error.message : String(error)}\n如果结果未知，请先查看刷新后的额度，再决定是否重试。`,
        });
      },
    );
  }

  return createPortal(
    <m.div
      key="reset-credits-dialog"
      className={styles.dialogBackdrop}
      role="presentation"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0, transition: { duration: 0.3, ease: 'easeIn' } }}
      transition={{ duration: 0.16, ease: 'easeOut' }}
      onMouseDown={(event) => {
        if (!busy && event.target === event.currentTarget) onClose();
      }}
    >
      <m.div
          ref={dialogRef}
          tabIndex={-1}
          className={styles.dialog}
          role="dialog"
          aria-modal="true"
          aria-labelledby="reset-credits-title"
          initial={{ opacity: 0, scale: 0.96, y: 10 }}
          animate={{ opacity: 1, scale: 1, y: 0 }}
          exit={{
            opacity: 0,
            scale: 0.96,
            y: 10,
            transition: { duration: 0.28, ease: 'easeIn' },
          }}
          transition={{ type: 'spring', stiffness: 460, damping: 34 }}
        >
          <button
            type="button"
            className={styles.dialogClose}
            onClick={onClose}
            disabled={busy}
            aria-label="关闭"
          >
            <X size={16} />
          </button>

          <header className={styles.dialogHeader}>
            <h2 id="reset-credits-title">Codex 重置卡</h2>
            <p className={styles.dialogSubtitle}>
              {loading
                ? `${accountName} · 正在读取重置卡…`
                : loadError
                  ? accountName
                  : `${accountName} · 可用 ${availableCount} 张`}
            </p>
          </header>

          {loading ? (
            <p className={styles.accountNote}>正在读取重置卡…</p>
          ) : loadError ? (
            <p className={styles.noticeDanger}>{loadError}</p>
          ) : phase === 'confirm' ? (
            <div className={styles.resetCreditConfirm}>
              <p className={styles.resetCreditWarning}>
                这会立即重置当前 5 小时与每周额度，并消耗 1 张重置卡。操作不可撤销。
              </p>
              <div className={styles.resetCreditRow}>
                <span className={styles.resetCreditMain}>
                  <span className={styles.resetCreditTitle}>
                    {selected ? creditTitle(selected) : '由服务端选择一张可用重置卡'}
                  </span>
                  <span className={styles.resetCreditMeta}>
                    {selected ? creditExpiry(selected) : '使用后不可撤回'}
                  </span>
                </span>
              </div>
            </div>
          ) : availableCount <= 0 ? (
            <p className={styles.accountNote}>当前没有可用的重置卡。</p>
          ) : (
            <>
              {details?.credits === null ? (
                <p className={styles.accountNote}>
                  当前 Codex CLI 只返回可用数量，无法列出具体卡片；使用时会由服务端选择一张。
                </p>
              ) : availableCredits.length > 0 ? (
                <>
                  <div className={styles.resetCreditList} role="list">
                    {availableCredits.map((credit) => (
                      <div className={styles.resetCreditRow} role="listitem" key={credit.id}>
                        <span className={styles.resetCreditMain}>
                          <span className={styles.resetCreditTitle}>{creditTitle(credit)}</span>
                          <span className={styles.resetCreditMeta}>
                            {creditExpiry(credit)}
                          </span>
                        </span>
                      </div>
                    ))}
                  </div>
                  <p className={styles.accountNote}>
                    使用时会优先选择最早到期的一张。
                  </p>
                </>
              ) : (
                <p className={styles.accountNote}>
                  明细中没有可用的重置卡（可能正在兑换）；如果数量仍有更新，请稍后刷新。
                </p>
              )}
            </>
          )}

          {notice ? (
            <p
              className={
                notice.kind === 'error' ? styles.noticeDanger : styles.resetCreditNotice
              }
              data-kind={notice.kind}
              role={notice.kind === 'error' ? 'alert' : 'status'}
            >
              {notice.text}
            </p>
          ) : null}

          <div className={styles.dialogActions}>
            {notice?.kind === 'success' || notice?.kind === 'info' ? (
              <button type="button" className={styles.button} onClick={onClose}>
                完成
              </button>
            ) : phase === 'confirm' ? (
              <>
                {notice?.kind === 'error' ? (
                  <button
                    type="button"
                    className={styles.buttonGhost}
                    onClick={onClose}
                    disabled={busy}
                  >
                    关闭
                  </button>
                ) : (
                  <button
                    type="button"
                    className={styles.buttonGhost}
                    onClick={cancelConfirm}
                    disabled={busy}
                  >
                    返回
                  </button>
                )}
                <button
                  type="button"
                  className={styles.dangerButton}
                  onClick={useCredit}
                  disabled={busy || !desktop}
                >
                  {busy
                    ? '正在使用…'
                    : notice?.kind === 'error'
                      ? '重试同一操作'
                      : '确认使用'}
                </button>
              </>
            ) : (
              <>
                <button type="button" className={styles.buttonGhost} onClick={onClose}>
                  关闭
                </button>
                <button
                  type="button"
                  className={styles.button}
                  onClick={beginConfirm}
                  disabled={busy || !desktop || availableCount <= 0 || loading || !!loadError}
                >
                  使用重置卡
                </button>
              </>
            )}
          </div>
      </m.div>
    </m.div>,
    document.body,
  );
}
