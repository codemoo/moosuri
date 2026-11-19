// WebKit may allow pinch scaling despite the viewport directive.
function preventTouchPageZoom(event: Event) {
  if (window.matchMedia("(pointer: coarse)").matches) event.preventDefault();
}
for (const gesture of ["gesturestart", "gesturechange"]) {
  document.addEventListener(gesture, preventTouchPageZoom, { passive: false, capture: true });
}
