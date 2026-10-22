// Backend error keys of the tool-call probe (harness_core::tool_probe).
// Texts are single quoted literals (the CLI compiles the English half in).
const tr = {
  'tool_probe.unreachable': 'Sunucuya ulaşılamadı; araç çağırma sınaması çalışmadı.',
  'tool_probe.no_server': 'Bu sunucu çalışmıyor ya da yönlendirici modunda.',
};

const en: Record<keyof typeof tr, string> = {
  'tool_probe.unreachable': 'The server could not be reached; the tool probe did not run.',
  'tool_probe.no_server': 'This server is not running or is in router mode.',
};

export default { tr, en };
