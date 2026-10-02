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

/**
 * Outcome of the bounded post-action observation the driver performs after a
 * successful action that raised the semantic revision floor.
 *
 * - `semantic_published`: the page published a snapshot at or above the floor.
 * - `semantic_pending`: the page reports in-flight semantic work, so the floor
 *   must stay raised until the publication arrives.
 * - `unproven`: the page gave no settled quiescence evidence; a successful
 *   click alone is not proof of a no-op, so the floor stays raised.
 * - `dom_only`: the semantic snapshot is settled and unchanged but the
 *   rendered DOM changed (DOM-only navigation).
 * - `confirmed_noop`: settled, unchanged semantic snapshot and unchanged DOM.
 */
export type PostActionObservation =
  | 'semantic_published'
  | 'semantic_pending'
  | 'unproven'
  | 'dom_only'
  | 'confirmed_noop';

const PENDING_QUIESCENCE_REASON_PREFIXES = ['operation_submitting', 'readiness_loading'];

function snapshotHasPendingSemanticWork(snapshot: UiSnapshotPayload): boolean {
  const reasons = Array.isArray(snapshot.quiescence?.reason_codes)
    ? (snapshot.quiescence?.reason_codes as unknown[]).map((value) => String(value ?? ''))
    : [];
  if (
    reasons.some((reason) =>
      PENDING_QUIESCENCE_REASON_PREFIXES.some((prefix) => reason.startsWith(prefix))
    )
  ) {
    return true;
  }
  const operations = Array.isArray(snapshot.operations) ? snapshot.operations : [];
  return operations.some(
    (operation) =>
      String((operation as { state?: unknown } | null)?.state ?? '').toLowerCase() ===
      'submitting'
  );
}

export function classifyPostActionObservation(
  requiredRevision: number | null | undefined,
  snapshot: UiSnapshotPayload | null | undefined,
  domBefore: string | null,
  domAfter: string | null
): PostActionObservation {
  if (!snapshot || typeof snapshot !== 'object') {
    return 'unproven';
  }
  const required = requiredRevision ?? 0;
  if (required > 0 && uiSnapshotRevision(snapshot) >= required) {
    return 'semantic_published';
  }
  if (snapshotHasPendingSemanticWork(snapshot)) {
    return 'semantic_pending';
  }
  const quiescenceState = String(snapshot.quiescence?.state ?? '').toLowerCase();
  // `busy` without pending semantic work only reflects a blocking modal that
  // was already part of the published snapshot; anything else is unproven.
  if (quiescenceState !== 'settled' && quiescenceState !== 'busy') {
    return 'unproven';
  }
  if (domBefore == null || domAfter == null) {
    return 'unproven';
  }
  return domBefore === domAfter ? 'confirmed_noop' : 'dom_only';
}
