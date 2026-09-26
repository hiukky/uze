// A recording, framed the way the landing page frames the hero one: the
// frame's own background is the app's, so a GIF that does not fill its box
// shows no border. `prefers-reduced-motion` gets the poster instead, and
// <source media> means only the matched file is ever fetched.
export function Demo({
  src,
  poster,
  alt,
  caption,
}: {
  src: string;
  poster: string;
  alt: string;
  caption?: string;
}) {
  // The prose column's own width: bleeding past it ran under the sidebar,
  // whose edge the content padding does not clear.
  return (
    <figure className="my-8">
      <div className="overflow-hidden rounded-md" style={{ background: '#0a0c0d' }}>
        {/* The margin is zeroed on the `picture`, which is what carries it:
            the docs typography gives media in the content a 2em block margin,
            and inside this frame that lands as 64px of the app's own
            background above and below — a gap that reads as the recording
            itself being padded, or as its height stretched. */}
        <picture style={{ display: 'block', margin: 0 }}>
          <source srcSet={poster} media="(prefers-reduced-motion: reduce)" />
          <img src={src} alt={alt} className="block h-auto w-full" style={{ margin: 0 }} />
        </picture>
      </div>
      {caption ? (
        <figcaption className="mt-2 text-center text-xs text-fd-muted-foreground">{caption}</figcaption>
      ) : null}
    </figure>
  );
}
