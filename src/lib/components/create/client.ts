import { invoke } from '$lib/ipc';
import type { GenerateToolRequest, ToolDraft, CoreTool, GeneratorUsage, GeneratorAttempt, GeneratorEstimate } from '$lib/tools-client';
import { t, type TKey } from '$lib/i18n.svelte';
export const categories = ['file', 'web', 'calculation', 'datetime', 'text', 'other'] as const;
export type Category = typeof categories[number];
export interface InventoryTool { name: string; description: string; category: Category; source: CoreTool['source']; server: string | null }
export type Status = 'queued' | 'building' | 'testing' | 'installing' | 'awaiting_approval' | 'installed' | 'failed';
export interface QueueItem { id: string; proposal: { name: string; description: string; category: Category }; status: Status; usage?: GeneratorUsage; history?: GeneratorAttempt[]; draft: ToolDraft | null; error: string | null }
export interface RunState { usage?: GeneratorUsage; estimate?: GeneratorEstimate; version: number; id: string; status: 'idle' | 'running' | 'paused' | 'stopped' | 'completed' | 'failed'; stage: string; count: number; inventory: InventoryTool[]; queue: QueueItem[]; logs: { at: number; stage: string; message: string }[]; error: string | null }
export const categoryLabel = (category: string) => t(('create.category_' + category) as TKey);
export const statusLabel = (status: string) => t(('create.status_' + status) as TKey);
export const inventory = () => invoke<InventoryTool[]>('tools_inventory');
export const state = () => invoke<RunState>('auto_tools_state');
export const start = (endpoint: GenerateToolRequest['endpoint'], count: number, resume = false) => invoke<RunState>('auto_tools_start', { endpoint, count, resume });
export const action = (action: string, id: string | null = null, order: string[] | null = null) => invoke<RunState>('auto_tools_action', { action, id, order });
export const retryFailed = () => action('retry_failed');

export const tokenEstimate = (request: GenerateToolRequest, count = 1, automatic = false) => invoke<GeneratorEstimate>('generator_token_estimate', { request, count, automatic });
