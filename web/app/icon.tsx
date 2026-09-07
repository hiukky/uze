import { ImageResponse } from 'next/og';

export const size = {
  width: 32,
  height: 32,
};
export const contentType = 'image/png';

// uze's mark: ❖ — the same glyph the nav and the footer carry. Drawn as
// geometry rather than as the character, because this renders through Satori,
// which needs a loaded font to have U+2756 and would otherwise emit tofu.
//
// The shape is a diamond quartered by an X: fill the diamond, then stroke the
// bounding box's two diagonals in the ground colour, which cuts the gaps.
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
        <svg width="24" height="24" viewBox="0 0 32 32">
          <path d="M16 2 L30 16 L16 30 L2 16 Z" fill="#8fd19e" />
          <path d="M3 3 L29 29 M29 3 L3 29" stroke="#0a0c0d" strokeWidth="2.6" />
        </svg>
      </div>
    ),
    size,
  );
}
