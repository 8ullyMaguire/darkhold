  export interface PanelAnchor {
    left: number;
    top: number;
  }

  /**
   * Keep a fixed panel inside the viewport on both axes. `place()` measures the
   * trigger, but a low trigger plus a tall panel used to put the last link below
   * the fold with no way to reach it. A panel taller than the viewport still
   * starts at the margin and scrolls inside its own box.
   */
  export function clampPanel(
    anchor: PanelAnchor,
    width: number,
    height: number,
    viewportWidth: number,
    viewportHeight: number,
    margin = 8,
  ): PanelAnchor {
    let left = anchor.left;
    if (left - width / 2 < margin) left = margin + width / 2;
    if (left + width / 2 > viewportWidth - margin) {
      left = viewportWidth - margin - width / 2;
    }
    let top = anchor.top;
    if (top + height > viewportHeight - margin) {
      top = Math.max(margin, viewportHeight - margin - height);
    }
    return { left, top };
  }

