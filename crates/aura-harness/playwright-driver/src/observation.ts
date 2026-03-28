import type { DriverSession, UiSnapshotPayload } from './contracts.js';

export function normalizeScreenText(value: unknown): string {
  return String(value ?? '')
    .split('\n')
    .map((line) => line.replace(/\s+/g, ' ').trim())
    .filter((line) => line.length > 0)
    .join('\n')
    .trim();
}

export function normalizeDomState(payload: unknown): { text: string; ids: Set<string> } {
  const ids = Array.isArray((payload as { ids?: unknown[] } | null)?.ids)
    ? ((payload as { ids: unknown[] }).ids ?? [])
        .map((value) => String(value ?? '').trim())
        .filter((value) => value.length > 0)
    : [];
  return {
    text: normalizeScreenText((payload as { text?: unknown } | null)?.text ?? ''),
    ids: new Set(ids)
  };
}

export function uiSnapshotRevision(snapshot: UiSnapshotPayload | null | undefined): number {
  const value = snapshot?.revision?.semantic_seq;
  return Number.isFinite(value) ? Number(value) : 0;
}

export function uiSnapshotRenderRevision(snapshot: UiSnapshotPayload | null | undefined): number {
  const value = snapshot?.revision?.render_seq;
  return Number.isFinite(value) ? Number(value) : 0;
}

export function uiStateStalenessReason(
  session: DriverSession,
  snapshot: UiSnapshotPayload | null
): string | null {
  if (!snapshot || typeof snapshot !== 'object') {
    return 'missing_snapshot';
  }
  const semanticRevision = uiSnapshotRevision(snapshot);
  if (semanticRevision <= 0) {
    return 'missing_semantic_revision';
  }
  const requiredRevision = session.requiredUiStateRevision ?? 0;
  if (requiredRevision > 0 && semanticRevision < requiredRevision) {
    return `required_revision_not_reached:${requiredRevision}`;
  }
  // Render heartbeat is a separate render-convergence signal, not a semantic
  // freshness gate. During browser rebinding and post-bootstrap publication,
  // the page-owned semantic snapshot may advance before the next
  // requestAnimationFrame publishes the matching heartbeat. Shared semantic
  // waits must accept the authoritative semantic snapshot in that window
  // instead of treating the older heartbeat as evidence that the snapshot is
  // stale.
  return null;
}
