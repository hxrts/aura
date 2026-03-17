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
  const heartbeatScreen =
    typeof session.renderHeartbeat?.screen === 'string' ? session.renderHeartbeat.screen : null;
  if (heartbeatScreen && snapshot.screen && heartbeatScreen !== snapshot.screen) {
    return `heartbeat_screen_mismatch:${heartbeatScreen}:${snapshot.screen}`;
  }
  const heartbeatRenderRevision = Number(session.renderHeartbeat?.render_seq ?? 0);
  const snapshotRenderRevision = uiSnapshotRenderRevision(snapshot);
  if (
    heartbeatRenderRevision > 0 &&
    snapshotRenderRevision > 0 &&
    heartbeatRenderRevision > snapshotRenderRevision
  ) {
    return `heartbeat_ahead:${heartbeatRenderRevision}:${snapshotRenderRevision}`;
  }
  return null;
}
