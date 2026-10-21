// Projects: chats, files and agent tasks grouped under a name and a folder (core `projects`).
import { invoke } from '#lib/ipc.js';

// Locale of the UI language (the page's `lang`), so this file stays free of app imports for its tests.
const uiDecimals = (value: number, digits: number) =>
  value.toLocaleString(globalThis.document?.documentElement.lang || 'en', { minimumFractionDigits: digits, maximumFractionDigits: digits, useGrouping: false });

export interface Project {
  id: string;
  name: string;
  description: string;
  folder: string | null;
  allowed_folders: string[];
  tools: string[];
  skills: string[];
  mcp_servers: string[];
  archived: boolean;
  created_at: number;
  updated_at: number;
  chat_count: number;
  output_count: number;
}

export type ProjectInput = Pick<Project, 'name' | 'description' | 'folder' | 'allowed_folders' | 'tools' | 'skills' | 'mcp_servers'>;

export type OutputKind = 'image' | 'audio' | 'text' | 'file';
export type OutputSourceKind = 'chat' | 'tool' | 'image' | 'speech' | 'agent' | 'manual';

export interface ProjectOutput {
  id: string;
  project_id: string;
  path: string;
  kind: OutputKind;
  source: OutputSourceKind;
  model: string | null;
  conversation_id: string | null;
  agent_task_id: string | null;
  tool: string | null;
  size: number;
  created_at: number;
  exists?: boolean;
}

export interface OutputSource {
  source: OutputSourceKind;
  model?: string | null;
  conversation_id?: string | null;
  agent_task_id?: string | null;
}

export interface DeleteReport {
  deleted_files: number;
  kept_files: number;
  unassigned_chats: number;
}

export interface AgentTask {
  id: string;
  model: string;
  prompt: string;
  started_at: number;
  finished_at: number | null;
  status: string;
  output: string;
  error: string | null;
  project_id: string | null;
}

export const projectApi = {
  list: (archived = false) => invoke<Project[]>('projects_list', { archived }),
  create: (input: ProjectInput) => invoke<Project>('project_create', { input }),
  update: (id: string, input: ProjectInput) => invoke<Project>('project_update', { id, input }),
  archive: (id: string, archived: boolean) => invoke<Project>('project_archive', { id, archived }),
  remove: (id: string, deleteFiles: boolean) => invoke<DeleteReport>('project_delete', { id, deleteFiles }),
  setChat: (conversationId: string, projectId: string | null) => invoke<void>('project_set_chat', { conversationId, projectId }),
  outputs: (id: string) => invoke<ProjectOutput[]>('project_outputs', { id }),
  forgetOutput: (id: string) => invoke<void>('project_output_remove', { id }),
  openOutput: (id: string) => invoke<void>('project_output_open', { id }),
  revealOutput: (id: string) => invoke<void>('project_output_reveal', { id }),
  revealFolder: (id: string) => invoke<void>('project_reveal_folder', { id }),
  saveOutput: (projectId: string, name: string, data: string, from: OutputSource) =>
    invoke<ProjectOutput>('project_save_output', { projectId, name, data, from }),
  exportZip: (id: string) => invoke<string | null>('project_export', { id }),
  startAgent: (id: string, prompt: string) => invoke<void>('project_agent_start', { id, prompt }),
  agentTasks: (id: string) => invoke<AgentTask[]>('project_agent_tasks', { id }),
  saveAgentOutput: (id: string, taskId: string) => invoke<ProjectOutput>('project_agent_save_output', { id, taskId }),
};

export function emptyInput(): ProjectInput {
  return { name: '', description: '', folder: null, allowed_folders: [], tools: [], skills: [], mcp_servers: [] };
}

export function inputOf(p: Project): ProjectInput {
  return {
    name: p.name,
    description: p.description,
    folder: p.folder,
    allowed_folders: [...p.allowed_folders],
    tools: [...p.tools],
    skills: [...p.skills],
    mcp_servers: [...p.mcp_servers],
  };
}

/** One path per line in the form; blank lines dropped. */
export function folderLines(text: string): string[] {
  return text.split(/\r?\n/).map((l) => l.trim()).filter(Boolean);
}

/** Last path component, for compact output rows. */
export function fileName(path: string): string {
  return path.split(/[/\\]/).filter(Boolean).at(-1) ?? path;
}

/**
 * Whether a project chat offers this tool: a project that attached tools offers only those, one
 * that attached MCP servers only their tools. A project without attachments changes nothing.
 */
export function projectAllowsTool(
  project: Pick<Project, 'tools' | 'mcp_servers'> | null | undefined,
  tool: { name: string; source?: string; server?: string | null },
): boolean {
  if (!project) return true;
  if (tool.source === 'mcp') return project.mcp_servers.length === 0 || project.mcp_servers.includes(tool.server ?? '');
  return project.tools.length === 0 || project.tools.includes(tool.name);
}

export function projectAllowsSkill(project: Pick<Project, 'skills'> | null | undefined, name: string): boolean {
  return !project || project.skills.length === 0 || project.skills.includes(name);
}

/** Bytes as KB/MB with one decimal. */
export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${uiDecimals(bytes / 1024, 1)} KB`;
  return `${uiDecimals(bytes / 1024 / 1024, 1)} MB`;
}
