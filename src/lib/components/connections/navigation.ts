import { Link2, Network, Plug, BookOpen, Code, Server, Download, Activity, FlaskConical, Route, Wrench } from '@lucide/svelte';
import type { SectionGroup } from '../common/sections';
export const CONNECTION_GROUPS: SectionGroup[] = [
  { label: 'conn.tab_clients', pages: [
    { id: 'config', label: 'intg.section_config', icon: Wrench },
    { id: 'guide', label: 'intg.section_guide', icon: BookOpen },
    { level: 'advanced', id: 'sdk', label: 'intg.section_sdk', icon: Code },
    { level: 'advanced', id: 'harness-mcp', label: 'conn.nav_harness_mcp', icon: Plug },
  ] },
  { label: 'link.group', pages: [
    { id: 'harness-link', label: 'link.page', icon: Link2 },
  ] },
  { label: 'conn.tab_endpoints', pages: [
    { id: 'endpoints', label: 'conn.nav_endpoint_list', icon: Network },
    { level: 'advanced', id: 'ollama', label: 'conn.nav_ollama', icon: Plug },
    { level: 'advanced', id: 'ports', label: 'conn.nav_ports', icon: Route },
  ] },
  { level: 'advanced', label: 'conn.tab_mcp', pages: [
    { level: 'advanced', id: 'servers', label: 'conn.nav_servers', icon: Server },
    { level: 'advanced', id: 'import', label: 'conn.mcp_import', icon: Download },
    { level: 'advanced', id: 'status', label: 'conn.nav_status_tools', icon: Activity },
  ] },
  { level: 'advanced', label: 'conn.tab_test', pages: [
    { level: 'advanced', id: 'workbench', label: 'conn.nav_workbench', icon: FlaskConical },
    { level: 'advanced', id: 'health', label: 'conn.nav_health', icon: Activity },
  ] },
];
export const CONNECTION_PAGES = CONNECTION_GROUPS.flatMap((g) => g.pages);
