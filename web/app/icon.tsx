import { ImageResponse } from 'next/og';

export const size = {
  width: 32,
  height: 32,
};
export const contentType = 'image/png';

// The same accent-dot mark used throughout the site (nav, hero eyebrow,
// diagram nodes) — near-black ground, one sage-green signal, per the TUI's
// own palette (src/ui.rs).
export default function Icon() {
  return new ImageResponse(
    (
      <div
        style={{
          width: '100%',
          height: '100%',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          background: '#0a0c0d',
          borderRadius: 7,
          border: '1px solid #1e1f20',
        }}
      >
        <div
          style={{
            width: 18,
            height: 18,
            borderRadius: '50%',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            border: '1px solid #2a3a30',
          }}
        >
          <div
            style={{
              width: 8,
              height: 8,
              borderRadius: '50%',
              background: '#8fd19e',
            }}
          />
        </div>
      </div>
    ),
    size,
  );
}
