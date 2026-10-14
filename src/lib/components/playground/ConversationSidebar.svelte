<script lang="ts">
  import { untrack } from 'svelte';
  import { toast } from 'svelte-sonner';
  import { t, type TKey } from '#lib/i18n.svelte.js';
  import { cn } from '#lib/utils.js';
  import { Button, buttonVariants } from '#lib/components/ui/button/index.js';
  import { Input } from '#lib/components/ui/input/index.js';
  import * as Dialog from '#lib/components/ui/dialog/index.js';
  import * as DropdownMenu from '#lib/components/ui/dropdown-menu/index.js';
  import * as Tooltip from '#lib/components/ui/tooltip/index.js';
  import {
    ChevronRight,
    Copy,
    Download,
    Folder as FolderIcon,
    FolderInput,
    FolderPlus,
    MoreHorizontal,
    Pencil,
    Pin,
    PinOff,
    Plus,
    Search,
    Star,
    StarOff,
    Trash2,
    Upload,
    X,
  } from '@lucide/svelte';
  import { convApi, type ConversationSummary, type Folder } from './conversations';
  import SectionLoading from '#lib/components/common/SectionLoading.svelte';

  interface Props {
    activeId: string | null;
    /** Bumped by the parent whenever the list may have changed. */
    refreshKey: number;
    readOnly?: boolean;
    onOpen: (id: string) => void;
    onNew: () => void;
    onDeleted: (id: string) => void;
    onRenamed: (id: string, title: string) => void;
  }

  let { activeId, refreshKey, readOnly = false, onOpen, onNew, onDeleted, onRenamed }: Props = $props();

  const COLLAPSED_KEY = 'harness_playground_collapsed_folders';

  let items = $state<ConversationSummary[]>([]);
  let folders = $state<Folder[]>([]);
  let loaded = $state(false);
  let query = $state('');
  let results = $state<ConversationSummary[] | null>(null);
  let searching = $state(false);
  let collapsed = $state<string[]>(loadCollapsed());

  let renamingId = $state<string | null>(null);
  let renameDraft = $state('');
  let renamingFolderId = $state<string | null>(null);
  let folderDraft = $state('');
  let creatingFolder = $state(false);
  let newFolderName = $state('');
  /** Conversation to move into the folder that is being created. */
  let moveAfterCreate = $state<string | null>(null);
  let confirm = $state<{ kind: 'conversation' | 'folder'; id: string; name: string } | null>(null);

  function loadCollapsed(): string[] {
    try {
      const v = JSON.parse(localStorage.getItem(COLLAPSED_KEY) ?? '[]');
      return Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : [];
    } catch {
      return [];
    }
  }

  function toggleFolder(id: string) {
    collapsed = collapsed.includes(id) ? collapsed.filter((x) => x !== id) : [...collapsed, id];
    try {
      localStorage.setItem(COLLAPSED_KEY, JSON.stringify(collapsed));
    } catch {
      // ignore
    }
  }

  function fail(e: unknown) {
    toast.error(t('pg.conv_error', { message: String(e) }));
  }

  async function load() {
    try {
      const [list, f] = await Promise.all([convApi.list({ limit: 1000 }), convApi.folders()]);
      items = list;
      folders = f;
    } catch (e) {
      fail(e);
    } finally {
      loaded = true;
    }
  }

  $effect(() => {
    void refreshKey;
    untrack(() => {
      load();
      if (query.trim()) runSearch(query);
    });
  });

  // ---- Search (FTS over titles and messages) ----
  let searchSeq = 0;
  async function runSearch(q: string) {
    const seq = ++searchSeq;
    searching = true;
    try {
      const list = await convApi.list({ query: q, limit: 100 });
      if (seq === searchSeq) results = list;
    } catch (e) {
      if (seq === searchSeq) fail(e);
    } finally {
      if (seq === searchSeq) searching = false;
    }
  }

  $effect(() => {
    const q = query.trim();
    if (!q) {
      searchSeq++;
      results = null;
      searching = false;
      return;
    }
    const timer = setTimeout(() => runSearch(q), 200);
    return () => clearTimeout(timer);
  });

  let folderIds = $derived(new Set(folders.map((f) => f.id)));
  let pinned = $derived(items.filter((c) => c.pinned));
  let unfiled = $derived(items.filter((c) => !c.pinned && (!c.folder_id || !folderIds.has(c.folder_id))));
  function inFolder(id: string) {
    return items.filter((c) => !c.pinned && c.folder_id === id);
  }

  function displayTitle(c: { title: string }) {
    return c.title.trim() || t('pg.conv_untitled');
  }

  function focusSelect(node: HTMLInputElement) {
    requestAnimationFrame(() => {
      node.focus();
      node.select();
    });
  }

  // ---- Conversation actions ----
  function beginRename(c: ConversationSummary) {
    renamingId = c.id;
    renameDraft = c.title;
  }

  async function commitRename() {
    const id = renamingId;
    if (!id) return;
    renamingId = null;
    const title = renameDraft.trim();
    const current = items.find((c) => c.id === id);
    if (!title || title === current?.title) return;
    try {
      await convApi.setTitle(id, title, 'user');
      onRenamed(id, title);
      await load();
    } catch (e) {
      fail(e);
    }
  }

  function renameKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      commitRename();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      renamingId = null;
    }
  }

  async function act(fn: () => Promise<unknown>) {
    try {
      await fn();
      await load();
      if (query.trim()) runSearch(query.trim());
    } catch (e) {
      fail(e);
    }
  }

  async function duplicate(c: ConversationSummary) {
    try {
      const copy = await convApi.duplicate(c.id, t('pg.conv_copy_title', { title: displayTitle(c) }));
      await load();
      onOpen(copy.id);
    } catch (e) {
      fail(e);
    }
  }

  async function exportConversation(c: ConversationSummary, format: 'json' | 'markdown') {
    const labels: Record<string, string> = {
      user: t('pg.role_user'),
      assistant: t('pg.role_assistant'),
      system: t('pg.system_prompt'),
      tool: t('pg.md_tool'),
      reasoning: t('pg.thinking'),
      tool_call: t('pg.md_tool_call'),
      result: t('pg.md_result'),
      error: t('pg.md_error'),
      summary: t('cmp.md_summary'),
      documents: t('pg.md_documents'),
    };
    try {
      const path = await convApi.exportFile(c.id, format, displayTitle(c), labels);
      if (path) toast.success(t('pg.conv_exported', { path }));
    } catch (e) {
      fail(e);
    }
  }

  async function importConversation() {
    try {
      const conv = await convApi.importDialog();
      if (!conv) return;
      toast.success(t('pg.conv_imported', { title: displayTitle(conv) }));
      await load();
      onOpen(conv.id);
    } catch (e) {
      fail(e);
    }
  }

  async function confirmDelete() {
    const c = confirm;
    confirm = null;
    if (!c) return;
    if (c.kind === 'conversation') {
      await act(() => convApi.remove(c.id));
      onDeleted(c.id);
    } else {
      await act(() => convApi.deleteFolder(c.id));
    }
  }

  // ---- Folders ----
  function beginCreateFolder(moveId: string | null = null) {
    creatingFolder = true;
    newFolderName = '';
    moveAfterCreate = moveId;
  }

  async function commitCreateFolder() {
    // Enter commits and removes the input, whose blur would commit a second folder.
    if (!creatingFolder) return;
    const name = newFolderName.trim();
    const moveId = moveAfterCreate;
    creatingFolder = false;
    moveAfterCreate = null;
    if (!name) return;
    await act(async () => {
      const f = await convApi.createFolder(name);
      if (moveId) await convApi.move(moveId, f.id);
    });
  }

  function createKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      commitCreateFolder();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      creatingFolder = false;
    }
  }

  async function commitFolderRename() {
    const id = renamingFolderId;
    renamingFolderId = null;
    const name = folderDraft.trim();
    if (!id || !name) return;
    await act(() => convApi.renameFolder(id, name));
  }

  function folderRenameKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      commitFolderRename();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      renamingFolderId = null;
    }
  }

  /** Keeps focus on an inline editor that a menu item just opened. */
  function keepEditorFocus(e: Event) {
    if (renamingId || renamingFolderId || creatingFolder) e.preventDefault();
  }

  // ---- Drag a conversation onto a folder (or back to the root) ----
  // Pointer events, not HTML5 drag and drop: with Tauri's native file drop enabled (used for
  // Playground documents), WebView2 on Windows never delivers HTML5 drop events. A pointer drag
  // never starts an OS drag either, so the file-drop handler sees nothing and vice versa.
  // Keyboard alternative: the row menu's "Move to folder".
  const DRAG_THRESHOLD = 5;
  const EDGE_SCROLL = 28;

  type DragState = { id: string; title: string; from: string | null; x: number; y: number };
  /** Drop target under the pointer: folder id, `null` = no folder, `undefined` = none. */
  let drag = $state<DragState | null>(null);
  let dropTarget = $state<string | null | undefined>(undefined);
  let pressed: { c: ConversationSummary; x: number; y: number; pointerId: number } | null = null;
  let suppressClick = false;
  let scrollEl = $state<HTMLElement | null>(null);
  let announce = $state('');

  function folderOf(c: ConversationSummary): string | null {
    return c.folder_id && folderIds.has(c.folder_id) ? c.folder_id : null;
  }

  function onRowPointerDown(e: PointerEvent, c: ConversationSummary) {
    if (readOnly || results !== null || e.button !== 0 || e.pointerType === 'touch' || drag) return;
    pressed = { c, x: e.clientX, y: e.clientY, pointerId: e.pointerId };
    window.addEventListener('pointermove', onDragMove, true);
    window.addEventListener('pointerup', onDragEnd, true);
    window.addEventListener('pointercancel', cancelDrag, true);
    window.addEventListener('keydown', onDragKey, true);
    window.addEventListener('blur', cancelDrag);
  }

  function targetAt(x: number, y: number): string | null | undefined {
    const el = document.elementFromPoint(x, y)?.closest<HTMLElement>('[data-drop-folder]');
    if (!el) return undefined;
    const v = el.dataset.dropFolder ?? '';
    return v === '' ? null : v;
  }

  function onDragMove(e: PointerEvent) {
    const p = pressed;
    if (!p || e.pointerId !== p.pointerId) return;
    if (!drag) {
      if (Math.hypot(e.clientX - p.x, e.clientY - p.y) < DRAG_THRESHOLD) return;
      drag = { id: p.c.id, title: displayTitle(p.c), from: folderOf(p.c), x: e.clientX, y: e.clientY };
      window.getSelection()?.removeAllRanges();
      document.body.style.cursor = 'grabbing';
    }
    e.preventDefault();
    drag.x = e.clientX;
    drag.y = e.clientY;
    dropTarget = targetAt(e.clientX, e.clientY);
    const box = scrollEl?.getBoundingClientRect();
    if (scrollEl && box) {
      if (e.clientY < box.top + EDGE_SCROLL) scrollEl.scrollBy(0, -8);
      else if (e.clientY > box.bottom - EDGE_SCROLL) scrollEl.scrollBy(0, 8);
    }
  }

  function endDrag() {
    pressed = null;
    drag = null;
    dropTarget = undefined;
    document.body.style.cursor = '';
    window.removeEventListener('pointermove', onDragMove, true);
    window.removeEventListener('pointerup', onDragEnd, true);
    window.removeEventListener('pointercancel', cancelDrag, true);
    window.removeEventListener('keydown', onDragKey, true);
    window.removeEventListener('blur', cancelDrag);
  }

  function cancelDrag() {
    if (drag) {
      // The click that may follow the release must not open the conversation.
      suppressClick = true;
      setTimeout(() => (suppressClick = false), 0);
    }
    endDrag();
  }

  function onDragKey(e: KeyboardEvent) {
    if (e.key !== 'Escape' || !drag) return;
    e.preventDefault();
    e.stopPropagation();
    cancelDrag();
  }

  function onDragEnd(e: PointerEvent) {
    const d = drag;
    if (!d || (pressed && e.pointerId !== pressed.pointerId)) {
      endDrag();
      return;
    }
    const target = targetAt(e.clientX, e.clientY);
    cancelDrag();
    if (target === undefined || target === d.from) return;
    const folderName = target === null ? t('pg.conv_no_folder') : (folders.find((f) => f.id === target)?.name ?? '');
    void act(async () => {
      await convApi.move(d.id, target);
      announce = t('pg.conv_moved_to', { title: d.title, folder: folderName });
    });
  }

  function onRowClick(id: string) {
    if (suppressClick) {
      suppressClick = false;
      return;
    }
    onOpen(id);
  }

  /** The dragged conversation would change folder if dropped on `target`. */
  function isDropHere(target: string | null) {
    return !!drag && dropTarget === target && drag.from !== target;
  }

  $effect(() => () => endDrag());
</script>

{#snippet menuItem(label: TKey, icon: typeof Pin, onSelect: () => void, destructive = false)}
  {@const Icon = icon}
  <DropdownMenu.Item onclick={onSelect} variant={destructive ? 'destructive' : 'default'}>
    <Icon class="size-3.5" />
    <span>{t(label)}</span>
  </DropdownMenu.Item>
{/snippet}

{#snippet row(c: ConversationSummary)}
  {@const active = c.id === activeId}
  <li class="group/row relative">
    {#if renamingId === c.id}
      <Input
        class="h-7 text-xs"
        bind:value={renameDraft}
        aria-label={t('pg.conv_rename')}
        onkeydown={renameKeydown}
        onblur={commitRename}
        {@attach focusSelect}
      />
    {:else}
      <button
        type="button"
        class={cn(
          'flex w-full min-w-0 cursor-pointer flex-col items-start gap-0.5 rounded-md py-1.5 pr-8 pl-2 text-left text-xs outline-none transition-colors select-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring/50',
          active ? 'bg-muted font-medium text-foreground' : 'text-foreground/85',
          drag?.id === c.id && 'opacity-50'
        )}
        aria-current={active ? 'page' : undefined}
        title={displayTitle(c)}
        draggable="false"
        ondragstart={(e) => e.preventDefault()}
        onpointerdown={(e) => onRowPointerDown(e, c)}
        onclick={() => onRowClick(c.id)}
      >
        <span class="flex w-full min-w-0 items-center gap-1.5">
          {#if c.favorite}<Star class="size-3 shrink-0 fill-current text-warning" aria-label={t('pg.conv_is_favorite')} />{/if}
          <span class="truncate">{displayTitle(c)}</span>
        </span>
        {#if c.snippet}
          <span class="line-clamp-2 w-full text-xs font-normal text-muted-foreground">{c.snippet}</span>
        {/if}
      </button>
      {#if !readOnly}
        <DropdownMenu.Root>
          <DropdownMenu.Trigger
            class={cn(
              buttonVariants({ variant: 'ghost', size: 'icon-xs' }),
              'absolute top-1 right-1 opacity-0 group-hover/row:opacity-100 focus-visible:opacity-100 data-[state=open]:opacity-100',
              active && 'opacity-100'
            )}
            aria-label={t('pg.conv_actions', { title: displayTitle(c) })}
          >
            <MoreHorizontal />
          </DropdownMenu.Trigger>
          <DropdownMenu.Content align="start" class="w-52" onCloseAutoFocus={keepEditorFocus}>
            {@render menuItem('pg.conv_rename', Pencil, () => beginRename(c))}
            {@render menuItem(c.pinned ? 'pg.conv_unpin' : 'pg.conv_pin', c.pinned ? PinOff : Pin, () =>
              act(() => convApi.setPinned(c.id, !c.pinned))
            )}
            {@render menuItem(c.favorite ? 'pg.conv_unfavorite' : 'pg.conv_favorite', c.favorite ? StarOff : Star, () =>
              act(() => convApi.setFavorite(c.id, !c.favorite))
            )}
            {@render menuItem('pg.conv_duplicate', Copy, () => duplicate(c))}
            <DropdownMenu.Sub>
              <DropdownMenu.SubTrigger>
                <FolderInput class="size-3.5" />
                <span>{t('pg.conv_move')}</span>
              </DropdownMenu.SubTrigger>
              <DropdownMenu.SubContent class="w-48">
                <DropdownMenu.Item disabled={!c.folder_id} onclick={() => act(() => convApi.move(c.id, null))}>
                  <span>{t('pg.conv_no_folder')}</span>
                </DropdownMenu.Item>
                {#each folders as f (f.id)}
                  <DropdownMenu.Item disabled={c.folder_id === f.id} onclick={() => act(() => convApi.move(c.id, f.id))}>
                    <FolderIcon class="size-3.5" />
                    <span class="truncate">{f.name}</span>
                  </DropdownMenu.Item>
                {/each}
                <DropdownMenu.Separator />
                <DropdownMenu.Item onclick={() => beginCreateFolder(c.id)}>
                  <FolderPlus class="size-3.5" />
                  <span>{t('pg.folder_new')}</span>
                </DropdownMenu.Item>
              </DropdownMenu.SubContent>
            </DropdownMenu.Sub>
            <DropdownMenu.Sub>
              <DropdownMenu.SubTrigger>
                <Download class="size-3.5" />
                <span>{t('pg.conv_export')}</span>
              </DropdownMenu.SubTrigger>
              <DropdownMenu.SubContent class="w-40">
                <DropdownMenu.Item onclick={() => exportConversation(c, 'json')}>JSON</DropdownMenu.Item>
                <DropdownMenu.Item onclick={() => exportConversation(c, 'markdown')}>Markdown</DropdownMenu.Item>
              </DropdownMenu.SubContent>
            </DropdownMenu.Sub>
            <DropdownMenu.Separator />
            {@render menuItem(
              'pg.conv_delete',
              Trash2,
              () => (confirm = { kind: 'conversation', id: c.id, name: displayTitle(c) }),
              true
            )}
          </DropdownMenu.Content>
        </DropdownMenu.Root>
      {/if}
    {/if}
  </li>
{/snippet}

{#snippet sectionTitle(label: string)}
  <h3 class="px-2 pt-3 pb-1 text-xs font-medium text-muted-foreground">{label}</h3>
{/snippet}

<nav class={cn('flex h-full min-h-0 flex-col bg-card', drag && 'select-none')} aria-label={t('pg.conv_sidebar')}>
  <div class="flex h-11 shrink-0 items-center gap-1 border-b border-border px-2">
    <Button variant="ghost" size="sm" class="min-w-0 flex-1 justify-start" onclick={onNew}>
      <Plus />
      <span class="truncate">{t('pg.new_chat')}</span>
    </Button>
    {#if !readOnly}
      <Tooltip.Root>
        <Tooltip.Trigger>
          {#snippet child({ props })}
            <Button {...props} variant="ghost" size="icon-sm" aria-label={t('pg.folder_new')} onclick={() => beginCreateFolder()}>
              <FolderPlus />
            </Button>
          {/snippet}
        </Tooltip.Trigger>
        <Tooltip.Content>{t('pg.folder_new')}</Tooltip.Content>
      </Tooltip.Root>
      <Tooltip.Root>
        <Tooltip.Trigger>
          {#snippet child({ props })}
            <Button {...props} variant="ghost" size="icon-sm" aria-label={t('pg.conv_import')} onclick={importConversation}>
              <Upload />
            </Button>
          {/snippet}
        </Tooltip.Trigger>
        <Tooltip.Content>{t('pg.conv_import')}</Tooltip.Content>
      </Tooltip.Root>
    {/if}
  </div>

  <div class="shrink-0 px-2 pt-2">
    <div class="relative">
      <Search class="pointer-events-none absolute top-1/2 left-2 size-3.5 -translate-y-1/2 text-muted-foreground" aria-hidden="true" />
      <Input
        type="search"
        class="h-7 pr-7 pl-7 text-xs"
        placeholder={t('pg.conv_search')}
        aria-label={t('pg.conv_search')}
        bind:value={query}
        onkeydown={(e) => {
          if (e.key === 'Escape' && query) {
            e.preventDefault();
            query = '';
          }
        }}
      />
      {#if query}
        <button
          type="button"
          class="absolute top-1/2 right-1.5 -translate-y-1/2 rounded-sm text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
          aria-label={t('pg.conv_search_clear')}
          onclick={() => (query = '')}
        >
          <X class="size-3.5" />
        </button>
      {/if}
    </div>
  </div>

  <div class="min-h-0 flex-1 overflow-y-auto px-2 pb-3" bind:this={scrollEl}>
    {#if creatingFolder}
      <div class="pt-2">
        <Input
          class="h-7 text-xs"
          placeholder={t('pg.folder_name')}
          aria-label={t('pg.folder_name')}
          bind:value={newFolderName}
          onkeydown={createKeydown}
          onblur={commitCreateFolder}
          {@attach focusSelect}
        />
      </div>
    {/if}

    {#if results !== null}
      {@render sectionTitle(t('pg.conv_results'))}
      {#if results.length === 0}
        <p class="px-2 py-1 text-xs text-muted-foreground" role="status">
          {searching ? t('pg.conv_searching') : t('pg.conv_no_results')}
        </p>
      {:else}
        <ul class="flex flex-col gap-0.5">
          {#each results as c (c.id)}{@render row(c)}{/each}
        </ul>
      {/if}
    {:else if !loaded}
      <SectionLoading shape="rows" rows={5} class="mt-2" label={t('sl.chats_label')} phases={[t('sl.chats_1'), t('sl.chats_2')]} />
    {:else}
      {#if pinned.length}
        {@render sectionTitle(t('pg.conv_pinned'))}
        <ul class="flex flex-col gap-0.5">
          {#each pinned as c (c.id)}{@render row(c)}{/each}
        </ul>
      {/if}

      {#if folders.length}
        {@render sectionTitle(t('pg.conv_folders'))}
        <ul class="flex flex-col gap-0.5">
          {#each folders as f (f.id)}
            {@const open = !collapsed.includes(f.id)}
            {@const list = inFolder(f.id)}
            <li
              class={cn(
                'group/folder rounded-md transition-colors',
                isDropHere(f.id) && 'bg-primary/10 ring-2 ring-primary/60 ring-inset'
              )}
              data-drop-folder={f.id}
            >
              <div class="relative">
                {#if renamingFolderId === f.id}
                  <Input
                    class="h-7 text-xs"
                    bind:value={folderDraft}
                    aria-label={t('pg.folder_rename')}
                    onkeydown={folderRenameKeydown}
                    onblur={commitFolderRename}
                    {@attach focusSelect}
                  />
                {:else}
                  <button
                    type="button"
                    class="flex w-full min-w-0 cursor-pointer items-center gap-1.5 rounded-md py-1.5 pr-8 pl-1.5 text-left text-xs text-foreground/85 outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring/50"
                    aria-expanded={open}
                    onclick={() => toggleFolder(f.id)}
                  >
                    <ChevronRight class={cn('size-3.5 shrink-0 transition-transform', open && 'rotate-90')} aria-hidden="true" />
                    <FolderIcon class="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
                    <span class="truncate">{f.name}</span>
                    <span class="ml-auto font-num text-xs text-muted-foreground">{list.length}</span>
                  </button>
                  {#if !readOnly}
                    <DropdownMenu.Root>
                      <DropdownMenu.Trigger
                        class={cn(
                          buttonVariants({ variant: 'ghost', size: 'icon-xs' }),
                          'absolute top-1/2 right-1 -translate-y-1/2 opacity-0 group-hover/folder:opacity-100 focus-visible:opacity-100 data-[state=open]:opacity-100'
                        )}
                        aria-label={t('pg.folder_actions', { name: f.name })}
                      >
                        <MoreHorizontal />
                      </DropdownMenu.Trigger>
                      <DropdownMenu.Content align="start" class="w-44" onCloseAutoFocus={keepEditorFocus}>
                        {@render menuItem('pg.folder_rename', Pencil, () => {
                          renamingFolderId = f.id;
                          folderDraft = f.name;
                        })}
                        {@render menuItem('pg.folder_delete', Trash2, () => (confirm = { kind: 'folder', id: f.id, name: f.name }), true)}
                      </DropdownMenu.Content>
                    </DropdownMenu.Root>
                  {/if}
                {/if}
              </div>
              {#if open}
                {#if list.length}
                  <ul class="ml-3 flex flex-col gap-0.5 border-l border-border pl-1.5">
                    {#each list as c (c.id)}{@render row(c)}{/each}
                  </ul>
                {:else}
                  <p class="ml-3 border-l border-border py-1 pl-3 text-xs text-muted-foreground">{t('pg.folder_empty')}</p>
                {/if}
              {/if}
            </li>
          {/each}
        </ul>
      {/if}

      {#if drag && drag.from !== null}
        <div
          class={cn(
            'mt-2 rounded-md border border-dashed border-border px-2 py-2 text-center text-xs text-muted-foreground transition-colors',
            isDropHere(null) && 'border-primary bg-primary/10 text-foreground'
          )}
          data-drop-folder=""
        >
          {t('pg.conv_drop_root')}
        </div>
      {/if}

      {#if unfiled.length}
        <div
          class={cn('rounded-md transition-colors', isDropHere(null) && 'bg-primary/10 ring-2 ring-primary/60 ring-inset')}
          data-drop-folder=""
        >
          {@render sectionTitle(t('pg.conv_recent'))}
          <ul class="flex flex-col gap-0.5">
            {#each unfiled as c (c.id)}{@render row(c)}{/each}
          </ul>
        </div>
      {:else if loaded && !pinned.length && !folders.length}
        <p class="px-2 pt-3 text-xs text-muted-foreground">{t('pg.conv_empty')}</p>
      {/if}
    {/if}
  </div>

  <p class="sr-only" role="status" aria-live="polite">{announce}</p>
</nav>

{#if drag}
  <div
    class="pointer-events-none fixed z-50 max-w-60 truncate rounded-md border border-border bg-popover px-2 py-1 text-xs text-popover-foreground shadow-md"
    style="left: {drag.x + 12}px; top: {drag.y + 8}px"
    aria-hidden="true"
  >
    {drag.title}
  </div>
{/if}

<Dialog.Root open={!!confirm} onOpenChange={(open) => !open && (confirm = null)}>
  <Dialog.Content class="sm:max-w-md">
    <Dialog.Header>
      <Dialog.Title>
        {confirm?.kind === 'folder' ? t('pg.folder_delete_title') : t('pg.conv_delete_title')}
      </Dialog.Title>
      <Dialog.Description>
        {confirm?.kind === 'folder'
          ? t('pg.folder_delete_desc', { name: confirm?.name ?? '' })
          : t('pg.conv_delete_desc', { title: confirm?.name ?? '' })}
      </Dialog.Description>
    </Dialog.Header>
    <Dialog.Footer class="mt-2 gap-2">
      <Button variant="outline" onclick={() => (confirm = null)}>{t('pg.cancel')}</Button>
      <Button variant="destructive" onclick={confirmDelete}>{t('pg.delete')}</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
