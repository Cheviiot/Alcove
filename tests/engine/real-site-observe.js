// SPDX-License-Identifier: GPL-3.0-only
// Diagnostic read-only measurements, sent over the existing private parent pipe.
(() => {
  const videos = [];
  const canvases = [];
  const inspect = (doc, depth = 0) => {
    if (depth > 4) return;
    for (const video of doc.querySelectorAll('video')) {
      if (videos.length >= 16) break;
      const quality = video.getVideoPlaybackQuality();
      videos.push({src: video.currentSrc, time: video.currentTime,
        paused: video.paused, ended: video.ended, ready: video.readyState,
        width: video.videoWidth, height: video.videoHeight,
        frames: quality.totalVideoFrames, dropped: quality.droppedVideoFrames,
        error: video.error?.code || null});
    }
    for (const canvas of doc.querySelectorAll('canvas')) {
      if (canvases.length >= 16) break;
      canvases.push({width: canvas.width, height: canvas.height,
        visible: canvas.getBoundingClientRect().width > 0});
    }
    for (const frame of doc.querySelectorAll('iframe')) {
      try { if (frame.contentDocument) inspect(frame.contentDocument, depth + 1); }
      catch (_) { /* Cross-origin documents are deliberately not bypassed. */ }
    }
  };
  inspect(document);
  console.log('ALCOVE_SITE_STATE:' + JSON.stringify({url: location.href,
    title: document.title, navigationStarted: performance.timeOrigin, ready: document.readyState, scale: devicePixelRatio,
    viewport: [innerWidth, innerHeight], scroll: [scrollX, scrollY],
    height: document.documentElement.scrollHeight,
    text: document.body?.innerText.slice(0, 1200), videos, canvases}));
})();
