/**
 * Lyra Landing Page — Utility Engine
 */

window.copyCli = function(text, btn) {
  navigator.clipboard.writeText(text).then(() => {
    const originalHTML = btn.innerHTML;
    btn.innerHTML = `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="#ffffff" stroke-width="2.5"><polyline points="20 6 9 17 4 12"/></svg>`;
    setTimeout(() => {
      btn.innerHTML = originalHTML;
    }, 2000);
  });
};

window.copyUri = function(btn) {
  const text = document.getElementById('uriBox').textContent.trim();
  navigator.clipboard.writeText(text).then(() => {
    const originalText = btn.textContent;
    btn.textContent = 'Copiato ✓';
    btn.style.background = '#ffffff';
    btn.style.color = '#000000';
    setTimeout(() => {
      btn.textContent = originalText;
      btn.style.background = '';
      btn.style.color = '';
    }, 2000);
  });
};
