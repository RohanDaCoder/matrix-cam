// Fake camera shim: a MediaStream whose video track emits animated frames.
// Used only by the headless e2e test (inject before the app boots).
(function () {
  const W = 640, H = 480;
  const canvas = document.createElement('canvas');
  canvas.width = W; canvas.height = H;
  const ctx = canvas.getContext('2d');
  const stream = canvas.captureStream(30);
  // animate a moving gradient so every frame differs
  let t = 0;
  function draw() {
    t += 0.15;
    const g = ctx.createLinearGradient(0, 0, W, H);
    g.addColorStop(0, `hsl(${t % 360}, 80%, 40%)`);
    g.addColorStop(1, `hsl(${(t * 2) % 360}, 80%, 60%)`);
    ctx.fillStyle = g;
    ctx.fillRect(0, 0, W, H);
    ctx.fillStyle = '#fff';
    for (let i = 0; i < 8; i++) {
      ctx.fillRect((t * 60 + i * 90) % W, (Math.sin(t + i) * 0.5 + 0.5) * H, 24, 24);
    }
    requestAnimationFrame(draw);
  }
  draw();
  const orig = navigator.mediaDevices.getUserMedia.bind(navigator.mediaDevices);
  navigator.mediaDevices.getUserMedia = function (cons) {
    return cons && cons.video ? Promise.resolve(stream) : orig(cons);
  };
})();
