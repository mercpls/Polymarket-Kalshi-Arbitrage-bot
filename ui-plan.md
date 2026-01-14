# Fashion-Forward UI Rewrite Plan

This plan outlines the "super slick" UI rewrite, transforming the arbitrage dashboard into a high-fashion, data-intelligent experience.

## Design Ethos
- **Typography**: Bold, oversized, heavy-weight fonts (Inter/Outfit).
- **Words**: Simple, provocative, minimal. (e.g., "PROFIT" instead of "Total Guaranteed Profit").
- **Palette**: High-contrast monochromatic (Black/White) with a single neon accent (Electric Blue).
- **Layout**: Spacious "streetwear" aesthetic. Generous padding, bold borders, unique data visualizations.

---

## Route Rewrites

### 1. Home Page (`/`) - THE OVERVIEW
**Title**: `THE HAUTE PROFIT`
- **Hero Section**: A massive, center-aligned display of `summary.all_time_pnl`. Font size > 120px. 
- **The "Lookbook"**: Instead of a standard grid, use large, vertical "Stat Columns" with simple labels (Exposure, Cost, Active).
- **Status Bar**: A sleek, bottom-fixed bar reflecting system health with a glowing neon accent.

### 2. Markets Page (`/markets`) - THE COLLECTIONS
**Title**: `MARKET DISCOVERY`
- **Unique Grid**: Use the `MarketOpportunityCard` but resize it to be massive. 2 columns instead of 4.
- **Visuals**: Add 3D-like hover effects on cards. Use large gradient text for the market spread.
- **Search**: A minimal, full-width search bar at the top that looks like a search for high-fashion runway shows.

### 3. Positions Page (`/positions`) - THE PORTFOLIO
**Title**: `ACTIVE ENSEMBLES`
- **The List**: Ditch the table. Use a list of horizontal, high-contrast rows.
- **Typography**: Each row starts with the market ID in a bold, small-cap font.
- **Status Icons**: Use abstract shapes (triangles, circles) instead of standard badges.
- **Interactivity**: Clicking a row "expands" it with a sliding animation to show leg details.

### 4. Trades Page (`/trades`) - THE ARCHIVE
**Title**: `THE LOGS`
- **The Feed**: A chronological stream of trade events.
- **Design**: Each event is a single line of text: `[14:42] BOUGHT 500 YES @ KALSHI`.
- **Contrast**: Alternating heavy and light font weights for readability and style.

---

## Intelligent Features
- **Smart Updates**: Numbers should "roll" or animate when they change via WebSocket.
- **Reactive Icons**: System icons should pulse or change color depth based on system "momentum" (e.g., higher arb frequency = faster pulse).
- **Responsive "Snap"**: The UI should feel snappy, with distinct, fast transitions between routes.

## Implementation Guide for Model
1.  **Fonts**: Import and set heavy font weights in `app.css`.
2.  **Components**: Use `StatCard` but apply `text-6xl` or `text-8xl` to the values.
3.  **Borders**: Use `border-2` or `border-4` for a "brutalist" fashion feel.
4.  **Simplicity**: Remove ALL unnecessary words. (e.g., "Total Unmatched Exposure" becomes "RISK").
