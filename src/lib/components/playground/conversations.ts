// Playground conversations stored in state.db (see harness_core::state): types mirroring
// the Rust API, thin `invoke` wrappers and mapping between stored and UI messages.

import { invoke } from '#lib/ipc.js';
import { sanitizeSettings, sanitizeToolCalls, type ChatMessage, type PgSettings, normalizeStats } from './types';
import { compareMetaOf } from './split';
import { ragTurnOf } from './rag';
import { imagesOfMeta } from './images';
import { audioOfMeta } from './audio';
import { compactionOf } from './compaction.svelte';

export type TitleSource = 'none' | 'fallback' | 'auto' | 'user';

export interface Folder {
  id: string;
  name: string;
  parent_id: string | null;
  sort: number;
  created_at: number;
}

export interface ConversationSummary {
  id: string;
  title: string;
  title_source: TitleSource;
  folder_id: string | null;
  pinned: boolean;
  favorite: boolean;
  created_at: number;
  updated_at: number;
  last_opened_at: number | null;
  model_alias: string | null;
  snippet: string | null;
}

export interface Conversation extends Omit<ConversationSummary, 'snippet'> {
  server_id: string | null;
  preset_id: string | null;
  system_prompt: string;
  settings: Record<string, unknown>;
  metadata: Record<string, unknown>;
  current_leaf_id: string | null;
  forked_from_conversation_id: string | null;
  forked_from_message_id: string | null;
}

export interface VariantInput {
  content: string;
  reasoning?: string | null;
  tool_calls?: unknown;
  stats?: unknown;
  finish_reason?: string | null;
  stopped?: boolean;
  tool_limit_reached?: boolean;
  error?: string | null;
  model_alias?: string | null;
  /** Client metadata; the split view stores `{ compare: { group, side, model, ... } }`. */
  meta?: Record<string, unknown> | null;
}

export interface Variant {
  id: string;
  message_id: string;
  content: string;
  reasoning: string | null;
  tool_calls: unknown;
  stats: unknown;
  finish_reason: string | null;
  stopped: boolean;
  tool_limit_reached: boolean;
  error: string | null;
  model_alias: string | null;
  created_at: number;
  meta?: Record<string, unknown> | null;
}

export interface PathMessage {
  id: string;
  parent_id: string | null;
  parent_variant_id: string | null;
  role: 'system' | 'user' | 'assistant' | 'tool';
  created_at: number;
  variant: Variant;
  variant_ids: string[];
  variant_index: number;
  sibling_ids: string[];
  sibling_index: number;
  /** Split turn: both sides of the shown variant's comparison group (absent otherwise). */
  compare?: Variant[];
  /**
   * Split turn regenerated: one variant id (side A) per comparison group, oldest first. The
   * shown group is the one `compare` belongs to.
   */
  compare_groups?: string[];
}

export interface ConversationView {
  conversation: Conversation;
  messages: PathMessage[];
}

export interface ListQuery {
  folder_id?: string | null;
  unfiled?: boolean;
  pinned_only?: boolean;
  favorites_only?: boolean;
  query?: string | null;
  limit?: number;
  offset?: number;
  /** Only the chats of this project. */
  project_id?: string | null;
}

export interface DbStatus {
  path: string;
  read_only: boolean;
  error: string | null;
}

// ---- API ----

export const convApi = {
  status: () => invoke<DbStatus>('conversation_db_status'),
  list: (query: ListQuery) => invoke<ConversationSummary[]>('list_conversations', { query }),
  create: (input: {
    title?: string;
    title_source?: TitleSource;
    folder_id?: string | null;
    server_id?: string | null;
    model_alias?: string | null;
    system_prompt?: string;
    settings?: Record<string, unknown>;
    preset_id?: string | null;
    metadata?: Record<string, unknown>;
    project_id?: string | null;
  }) => invoke<Conversation>('create_conversation', { input }),
  open: (id: string) => invoke<ConversationView>('open_conversation', { id }),
  get: (id: string) => invoke<ConversationView>('get_conversation', { id }),
  lastOpened: () => invoke<string | null>('last_opened_conversation'),
  setTitle: (id: string, title: string, source: TitleSource, onlyIfSource?: TitleSource) =>
    invoke<boolean>('set_conversation_title', { id, title, source, onlyIfSource: onlyIfSource ?? null }),
  update: (
    id: string,
    patch: {
      system_prompt?: string;
      settings?: Record<string, unknown>;
      model_alias?: string;
      server_id?: string;
      preset_id?: string;
      metadata?: Record<string, unknown>;
    }
  ) => invoke<Conversation>('update_conversation', { id, patch }),
  setPinned: (id: string, pinned: boolean) => invoke<void>('set_conversation_pinned', { id, pinned }),
  setFavorite: (id: string, favorite: boolean) => invoke<void>('set_conversation_favorite', { id, favorite }),
  move: (id: string, folderId: string | null) => invoke<void>('move_conversation', { id, folderId }),
  remove: (id: string) => invoke<void>('delete_conversation', { id }),
  duplicate: (id: string, title?: string) => invoke<Conversation>('duplicate_conversation', { id, title: title ?? null }),
  fork: (messageId: string, title?: string) => invoke<Conversation>('fork_conversation', { messageId, title: title ?? null }),
  append: (input: {
    conversation_id: string;
    parent_id: string | null;
    role: 'user' | 'assistant';
    variant: VariantInput;
    id?: string;
    variant_id?: string;
    /** Attach to the parent as a whole (continuation of a split turn). */
    any_parent_variant?: boolean;
  }) => invoke<PathMessage>('append_conversation_message', { input }),
  addVariant: (messageId: string, variant: VariantInput, variantId?: string) =>
    invoke<PathMessage>('add_message_variant', { messageId, variant, variantId: variantId ?? null }),
  updateVariant: (variantId: string, variant: VariantInput) => invoke<void>('update_message_variant', { variantId, variant }),
  selectVariant: (messageId: string, variantId: string) =>
    invoke<ConversationView>('select_message_variant', { messageId, variantId }),
  selectBranch: (messageId: string) => invoke<ConversationView>('select_conversation_branch', { messageId }),
  /** Split view: continue with one side of every split turn on the path. */
  adoptSide: (conversationId: string, side: 'a' | 'b') =>
    invoke<ConversationView>('adopt_compare_side', { conversationId, side }),
  editUserMessage: (messageId: string, content: string, newMessageId?: string) =>
    invoke<PathMessage>('edit_user_message', { messageId, content, newMessageId: newMessageId ?? null }),
  folders: () => invoke<Folder[]>('list_conversation_folders'),
  createFolder: (name: string, parentId?: string | null) =>
    invoke<Folder>('create_conversation_folder', { name, parentId: parentId ?? null }),
  renameFolder: (id: string, name: string) => invoke<void>('rename_conversation_folder', { id, name }),
  deleteFolder: (id: string) => invoke<void>('delete_conversation_folder', { id }),
  importLegacy: (importData: {
    conversation: { title?: string; title_source?: TitleSource; system_prompt?: string; settings?: Record<string, unknown>; model_alias?: string | null };
    messages: { role: 'user' | 'assistant'; variant: VariantInput }[];
  }) => invoke<string | null>('import_legacy_playground', { import: importData }),
  importPlaygroundState: (importData: {
    conversation: {
      conversation: {
        title?: string;
        title_source?: TitleSource;
        system_prompt?: string;
        settings?: Record<string, unknown>;
        model_alias?: string | null;
      };
      messages: { role: 'user' | 'assistant'; variant: VariantInput }[];
    } | null;
    values: { key: string; value: string }[];
  }) =>
    invoke<{ conversation_verified: boolean; verified_keys: string[] }>('import_playground_state', {
      import: importData,
    }),
  exportFile: (id: string, format: 'json' | 'markdown', fileName: string, labels?: Record<string, string>) =>
    invoke<string | null>('export_conversation', { id, format, fileName, labels: labels ?? null }),
  importDialog: () => invoke<Conversation | null>('import_conversation_dialog'),
  /** Saves a rendered (sanitized) diagram SVG through a save dialog; `null` if cancelled. */
  exportDiagramSvg: (svg: string, fileName: string) => invoke<string | null>('export_diagram_svg', { svg, fileName }),
  /** Saves a diagram drawn on a canvas as PNG through a save dialog; `null` if cancelled. */
  exportDiagramPng: (png: Uint8Array, fileName: string) =>
    invoke<string | null>('export_diagram_png', { png: Array.from(png), fileName }),
  /** "Edit from here": the path ends before this user message (the old continuation stays as a branch). */
  rewindBefore: (messageId: string) => invoke<ConversationView>('rewind_conversation_before', { messageId }),
};

// ---- Mapping ----

/** A stored earlier round (`meta.rounds`, see `ChatMessage.priorRounds`) → UI message. */
function roundOf(raw: unknown, index: number, parentId: string | null, createdAt?: number): ChatMessage | null {
  if (!raw || typeof raw !== 'object') return null;
  const r = raw as Record<string, unknown>;
  return {
    id: `round-${index}`,
    role: 'assistant',
    content: typeof r.content === 'string' ? r.content : '',
    reasoning: typeof r.reasoning === 'string' && r.reasoning ? r.reasoning : undefined,
    stats: normalizeStats(r.stats, createdAt),
    reasoningMs: reasoningMsOf(r),
    error: typeof r.error === 'string' ? r.error : undefined,
    stopped: r.stopped === true || undefined,
    finishReason: typeof r.finish_reason === 'string' ? r.finish_reason : undefined,
    contextSize: typeof r.context_size === 'number' && r.context_size > 0 ? r.context_size : undefined,
    toolCalls: sanitizeToolCalls(r.tool_calls),
    toolLimitReached: r.tool_limit_reached === true || undefined,
    parentId,
  };
}

/** Earlier rounds stored in a variant's `meta.rounds` (a split-view side's tool loop). */
export function roundsOf(meta: unknown, parentId: string | null, createdAt?: number): ChatMessage[] | undefined {
  const raw = (meta as { rounds?: unknown } | null | undefined)?.rounds;
  if (!Array.isArray(raw)) return undefined;
  const rounds = raw.map((r, i) => roundOf(r, i, parentId, createdAt)).filter((m): m is ChatMessage => !!m);
  return rounds.length ? rounds : undefined;
}

/** An earlier round as stored in `meta.rounds`. */
export function roundInput(m: ChatMessage): Record<string, unknown> {
  return {
    content: m.content,
    reasoning: m.reasoning || null,
    tool_calls: m.toolCalls?.length ? m.toolCalls : null,
    stats: m.stats ?? null,
    finish_reason: m.finishReason ?? null,
    context_size: m.contextSize ?? null,
    reasoning_ms: m.reasoningMs ?? null,
    stopped: !!m.stopped,
    tool_limit_reached: !!m.toolLimitReached,
    error: m.error ?? null,
  };
}

/** A stored side of a split turn → UI message. */
function fromSide(p: PathMessage, v: Variant): ChatMessage | null {
  const meta = compareMetaOf(v.meta);
  if (!meta) return null;
  return {
    id: p.id,
    role: 'assistant',
    content: v.content,
    reasoning: v.reasoning ?? undefined,
    reasoningMs: reasoningMsOf(v.meta),
    stats: normalizeStats(v.stats, v.created_at),
    error: v.error ?? undefined,
    stopped: v.stopped || undefined,
    finishReason: v.finish_reason ?? undefined,
    contextSize: contextOfMeta(v.meta),
    toolCalls: sanitizeToolCalls(v.tool_calls),
    toolLimitReached: v.tool_limit_reached || undefined,
    variantId: v.id,
    parentId: p.parent_id,
    rag: ragTurnOf(v.meta),
    priorRounds: roundsOf(v.meta, p.parent_id, v.created_at),
    compareSide: meta.side,
    compareMeta: meta,
  };
}

/** A stored compaction marker (a `system` message with `meta.compaction`). */
function isMarker(p: PathMessage): boolean {
  return p.role === 'system' && !!compactionOf(p.variant.meta);
}

/** Stored path message → UI message (other system and tool rows are not shown; callers filter). */
export function fromPath(p: PathMessage): ChatMessage {
  const v = p.variant;
  if (isMarker(p)) {
    return {
      id: p.id,
      role: 'compaction',
      content: v.content,
      compaction: compactionOf(v.meta),
      variantId: v.id,
      parentId: p.parent_id,
    };
  }
  const sides = (p.compare ?? []).map((c) => fromSide(p, c)).filter((m): m is ChatMessage => !!m);
  return {
    id: p.id,
    role: p.role === 'user' ? 'user' : 'assistant',
    content: v.content,
    reasoning: v.reasoning ?? undefined,
    reasoningMs: reasoningMsOf(v.meta),
    stats: normalizeStats(v.stats, v.created_at),
    error: v.error ?? undefined,
    stopped: v.stopped || undefined,
    finishReason: v.finish_reason ?? undefined,
    contextSize: contextOfMeta(v.meta),
    toolCalls: sanitizeToolCalls(v.tool_calls),
    toolLimitReached: v.tool_limit_reached || undefined,
    variantId: v.id,
    parentId: p.parent_id,
    rag: ragTurnOf(v.meta),
    images: p.role === 'user' ? imagesOfMeta(v.meta) : undefined,
    audio: p.role === 'user' ? audioOfMeta(v.meta) : undefined,
    priorRounds: roundsOf(v.meta, p.parent_id, v.created_at),
    variantIds: p.variant_ids,
    variantIndex: p.variant_index,
    siblingIds: p.sibling_ids,
    siblingIndex: p.sibling_index,
    compareSides: sides.length >= 2 ? sides : undefined,
    compareNav: compareNavOf(p),
  };
}

/** Navigation between regenerated pairs of a split turn (only when there is more than one). */
function compareNavOf(p: PathMessage): ChatMessage['compareNav'] {
  const ids = p.compare_groups ?? [];
  if (ids.length < 2) return undefined;
  const shown = p.compare?.[0]?.id;
  const index = shown ? ids.indexOf(shown) : -1;
  return { index: Math.max(0, index), count: ids.length, ids };
}

export function viewMessages(view: ConversationView): ChatMessage[] {
  return view.messages.filter((m) => m.role === 'user' || m.role === 'assistant' || isMarker(m)).map(fromPath);
}

/** UI message → stored variant (transient fields are dropped). */
export function toVariantInput(m: ChatMessage, model?: string): VariantInput {
  return {
    content: m.content,
    reasoning: m.reasoning || null,
    tool_calls: m.toolCalls?.length ? m.toolCalls : null,
    stats: m.stats ?? null,
    finish_reason: m.finishReason ?? null,
    stopped: !!m.stopped,
    tool_limit_reached: !!m.toolLimitReached,
    error: m.error ?? null,
    model_alias: model || null,
    ...(m.reasoningMs !== undefined || m.contextSize || m.rag || m.priorRounds?.length || m.images?.length || m.audio?.length
      ? { meta: { ...(m.reasoningMs !== undefined ? { reasoning_ms: m.reasoningMs } : {}), ...(m.contextSize ? { context_size: m.contextSize } : {}), ...(m.rag ? { rag: m.rag } : {}), ...(m.priorRounds?.length ? { rounds: m.priorRounds.map(roundInput) } : {}), ...(m.images?.length ? { images: m.images } : {}), ...(m.audio?.length ? { audio: m.audio } : {}) } }
      : {}),
  };
}

function contextOfMeta(meta: unknown): number | undefined {
  const n = (meta as { context_size?: unknown } | null)?.context_size;
  return typeof n === 'number' && Number.isFinite(n) && n > 0 ? n : undefined;
}

/** Settings snapshot kept per conversation (system prompt and model have their own columns). */
export function settingsSnapshot(s: PgSettings): Record<string, unknown> {
  const { systemPrompt: _sp, model: _m, ...rest } = s;
  return rest as Record<string, unknown>;
}

/** Restores a conversation's settings on top of the current ones. */
export function settingsFromConversation(c: Conversation, current: PgSettings): PgSettings {
  const base: Partial<PgSettings> = { ...current, workdir: '' };
  if (!('request' in c.settings)) {
    // Stored before presets: its own flat sampling keys, not the current request.
    delete base.request;
    delete base.presetId;
  }
  const restored = sanitizeSettings({ ...base, ...c.settings });
  restored.systemPrompt = c.system_prompt;
  // The preset column wins over the snapshot (conversations stored before presets have none).
  if (c.preset_id) restored.presetId = c.preset_id;
  restored.model = current.model;
  return restored;
}

export { cleanGeneratedTitle, fallbackTitle } from './titles';

export { turnRoot } from './branching';

function reasoningMsOf(meta: unknown): number | undefined {
  const n = (meta as { reasoning_ms?: unknown } | null)?.reasoning_ms;
  return typeof n === 'number' && Number.isFinite(n) && n >= 0 ? n : undefined;
}
