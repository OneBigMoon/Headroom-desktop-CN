import { useEffect, useState } from "react";

/** Rotated through during the long silent LLM call so the line keeps moving.
 * Index 0 of the cycle is the real step ("Analyzing with Claude Code"), so
 * the backend name resurfaces once per loop. */
const LEARN_ANALYZING_PHRASES = [
  "Reading through your sessions",
  "Looking for recurring friction",
  "Spotting repeated failures",
  "Weighing which lessons generalize",
  "Distilling patterns worth keeping",
  "Drafting memory updates"
];

const TYPE_INTERVAL_MS = 18;
const ROTATE_INTERVAL_MS = 7000;

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
 * show the scan is still alive. The full phrase sits in a visually-hidden
 * span so aria-live announces it once, not per typed character. */
export function LearnScanStatusLine({
  step,
  elapsedSeconds
}: {
  step?: string | null;
  elapsedSeconds?: number | null;
}) {
  const analyzing = step?.startsWith("Analyzing with") ?? false;
  const [variant, setVariant] = useState(0);
  useEffect(() => {
    if (!analyzing) {
      setVariant(0);
      return;
    }
    const id = window.setInterval(
      () => setVariant((current) => (current + 1) % (LEARN_ANALYZING_PHRASES.length + 1)),
      ROTATE_INTERVAL_MS
    );
    return () => window.clearInterval(id);
  }, [analyzing]);
  const phrase =
    analyzing && variant > 0
      ? LEARN_ANALYZING_PHRASES[variant - 1]
      : step ?? "Scanning sessions";
  const typed = useTypewriter(phrase);
  return (
    <>
      <span aria-hidden="true">
        {typed}
        {typeof elapsedSeconds === "number" ? ` · ${elapsedSeconds}s` : ""}
      </span>
      <span className="visually-hidden">{phrase}</span>
    </>
  );
}
