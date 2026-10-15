/** Shared space budget in CSS pixels, including the shell's 1px content border. */
export const MIN_CONTENT_WIDTH = 480;

/**
 * Content width below which a page's section navigation becomes a drawer. Higher than
 * MIN_CONTENT_WIDTH: at a high display scale (200% on a 1080p screen is 960 CSS px) the main
 * navigation and a docked section column would otherwise take almost half the window.
 */
export const SECTION_DRAWER_CONTENT_WIDTH = 600;

/** @param {{viewport: number, mainWidth: number, mainRail: number, mainCollapsed: boolean, sectionWidth?: number, sectionRail?: number}} input */
export function sidebarLayout({ viewport, mainWidth, mainRail, mainCollapsed, sectionWidth = 0, sectionRail = 0 }) {
  const preferredMain = mainCollapsed ? mainRail : mainWidth;
  const forcedMainRail = preferredMain + sectionRail + MIN_CONTENT_WIDTH + 1 > viewport;
  const navWidth = forcedMainRail ? mainRail : preferredMain;
  const sectionDrawer = sectionWidth > 0 && navWidth + sectionWidth + SECTION_DRAWER_CONTENT_WIDTH + 1 > viewport;
  return { navWidth, forcedMainRail, sectionDrawer };
}
