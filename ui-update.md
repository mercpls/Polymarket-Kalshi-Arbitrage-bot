# UI Rewrite Implementation Plan

## Goal
Rewrite the dashboard UI to be "super slick and intelligent" using shadcn/ui, directly reflecting the backend Rust data structures.

## Proposed Changes

### Dashboard Setup
- Initialize `shadcn/ui`.
- Install dependencies: `class-variance-authority`, `clsx`, `lucide-react`, `tailwind-merge`, `tailwindcss-animate`.

### Components
Install the following shadcn components via CLI:
- `accordion`, `alert`, `avatar`, `badge`, `breadcrumb`, `button`, `card`, `checkbox`, `collapsible`, `command`, `dialog`, `dropdown-menu`, `form`, `hover-card`, `input`, `label`, `popover`, `progress`, `scroll-area`, `select`, `separator`, `sheet`, `skeleton`, `slider`, `switch`, `table`, `tabs`, `textarea`, `toast`, `toggle`, `tooltip`.

### New Structure
- **Types**: Define TypeScript interfaces in `dashboard/app/types/api.ts` matching `src/web/types.rs`.
- **Layout**: New sidebar and header components.
- **Components**: Intelligent data-driven components (StatCard, PositionsTable, MarketOpportunityCard).
- **Refactor**: Update home route to use new components.

## Verification Plan
- Build verification: `npm run build` in `dashboard/`.
- Manual verification: Visual inspection via `npm run dev`.
