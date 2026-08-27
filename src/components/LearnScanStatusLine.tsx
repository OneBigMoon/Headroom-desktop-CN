import { useEffect, useState } from "react";
import { useI18n, type TranslationKey } from "../lib/i18n";

/** Rotated through during the long silent LLM call so the line keeps moving.
 * Index 0 of the cycle is the real step ("Analyzing with Claude Code"), so
 * the backend name resurfaces once per loop. */
const LEARN_ANALYZING_KEYS: TranslationKey[] = [
  "learn.scan.reading", "learn.scan.friction", "learn.scan.failures",
  "learn.scan.signal", "learn.scan.patterns", "learn.scan.drafting"
];

const TYPE_INTERVAL_MS = 18;
const ROTATE_INTERVAL_MS = 7000;

/** Width reserved for the line: longest phrase plus room for the timer. In ch
 * so it tracks the font; ch over-measures proportional lowercase, which is the
 * safe direction for a floor. Reserving it up front is what keeps the refresh
 * button and learning pills from reflowing on every typed character and every
 * phrase rotation. */
const RESERVED_CH =
  34;

/** Types `text` out character by character, restarting when `text` changes. */
function useTypewriter(text: string): string {
  const [length, setLength] = useState(0);
  useEffect(() => {
    setLength(0);
    if (!text) {
      return;
    }
    let current = 0;
    const id = window.setInterval(() => {
      current += 1;
      setLength(current);
      if (current >= text.length) {
        window.clearInterval(id);
      }
    }, TYPE_INTERVAL_MS);
    return () => window.clearInterval(id);
  }, [text]);
  return text.slice(0, length);
}

/** Live status of a running scan: the current CLI step typed out in place of
 * the static "Scanning sessions" label. The analysis step is one LLM call
 * with minutes of silence, so it cycles through LEARN_ANALYZING_PHRASES to
 * show the scan is still alive. Layout is jitter-free by construction: the
 * container reserves RESERVED_CH up front and the timer is pinned to its
 * right edge, so only the appearing characters change. The full phrase sits
 * in a visually-hidden span so aria-live announces it once, not per typed
 * character. */
export function LearnScanStatusLine({
  step,
  elapsedSeconds
}: {
  step?: string | null;
  elapsedSeconds?: number | null;
}) {
  const { t } = useI18n();
  const analyzing = step?.startsWith("Analyzing with") ?? false;
  const [variant, setVariant] = useState(0);
  useEffect(() => {
    if (!analyzing) {
      setVariant(0);
      return;
    }
    const id = window.setInterval(
      () => setVariant((current) => (current + 1) % (LEARN_ANALYZING_KEYS.length + 1)),
      ROTATE_INTERVAL_MS
    );
    return () => window.clearInterval(id);
  }, [analyzing]);
  const phrase =
    analyzing && variant > 0
      ? t(LEARN_ANALYZING_KEYS[variant - 1])
      : step?.startsWith("Analyzing with ")
        ? t("learn.scan.analyzing", { name: step.slice("Analyzing with ".length) })
        : step === "Reading sessions"
          ? t("learn.scan.readingSessions")
          : step ?? t("learn.scan.scanning");
  const typed = useTypewriter(phrase);
  return (
    <>
      <span
        aria-hidden="true"
        className="learn-scan-status"
        style={{ minWidth: `${RESERVED_CH}ch` }}
      >
        <span className="learn-scan-status__text">{typed}</span>
        {typeof elapsedSeconds === "number" ? (
          <span className="learn-scan-status__timer">{t("learn.scan.seconds", { count: elapsedSeconds })}</span>
        ) : null}
      </span>
      <span className="visually-hidden">{phrase}</span>
    </>
  );
}
