/** Shared space budget in CSS pixels, including the shell's 1px content border. */
export const MIN_CONTENT_WIDTH = 480;

/** @param {{viewport: number, mainWidth: number, mainRail: number, mainCollapsed: boolean, sectionWidth?: number, sectionRail?: number}} input */
export function sidebarLayout({ viewport, mainWidth, mainRail, mainCollapsed, sectionWidth = 0, sectionRail = 0 }) {
  const preferredMain = mainCollapsed ? mainRail : mainWidth;
  const forcedMainRail = preferredMain + sectionRail + MIN_CONTENT_WIDTH + 1 > viewport;
  const navWidth = forcedMainRail ? mainRail : preferredMain;
  const sectionDrawer = sectionWidth > 0 && navWidth + sectionWidth + MIN_CONTENT_WIDTH + 1 > viewport;
  return { navWidth, forcedMainRail, sectionDrawer };
}
