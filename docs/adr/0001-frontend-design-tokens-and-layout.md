# Frontend Design Tokens and Layout Architecture

## Context

Rulist previously used a legacy web styling configuration based on hardcoded gray backgrounds (`#f7f8fa`), stacked sticky headers (`Header` at 60px and `Nav` at 40px) occupying 100px of vertical space, a layout hack in `Nav.tsx` (`99vw` with negative margins to span viewport width), and large list item padding with permanently high-contrast action menus creating visual clutter across extensive file listings.

## Decision

1. **Design Tokens & Aesthetic Direction**:
   - Upgraded global theme tokens in `web/src/app/theme.ts` to a modern Zinc/Slate cool-gray palette (`#f8fafc` for light theme, `#09090b` for dark theme).
   - Applied subtle hairline borders (`rgba(0, 0, 0, 0.05-0.08)` and `rgba(255, 255, 255, 0.06-0.08)`), refined focus rings, smooth transitions, and standard typography smoothing.
   - Retained the existing Hope UI and Stitches runtime architecture with zero new dependencies, strictly honoring project minimalism.

2. **Top Chrome Architecture**:
   - Compressed the sticky stack height: `Header` reduced from 60px to 50px; `Nav` reduced from 40px to 36px.
   - Introduced semi-transparent backdrop blur (`backdrop-filter: blur(12px)`) across sticky bars so content scrolls smoothly underneath.
   - Removed the `99vw` full-width hack in `Nav.tsx`, converting the breadcrumb into a clean, floating responsive bar.
   - Aligned the sticky `ListTitle` offset dynamically (`86px` inside directories, `50px` at root).

3. **File Explorer Work Area & Micro-Interactions**:
   - Reduced `ListItem` row padding to `py="$2_5"` (~42px target row height) for improved information density.
   - Implemented progressive disclosure for row actions on desktop: the three-dot menu rests at subtle opacity and smoothly elevates to full opacity on row hover or when open, while maintaining immediate accessibility on mobile.
