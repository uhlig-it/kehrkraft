// Drag-and-drop reordering for the apartments table (htmx-powered).
//
// Only the drag handle starts a drag. Rows are reordered live as the pointer
// moves; on drop an `end` event is dispatched, which the tbody's
// `hx-trigger="end"` turns into a POST of the hidden `item` inputs (in their
// new DOM order). The server responds with the re-rendered tbody, which htmx
// swaps in and re-initializes via hx-on:load.
function initApartmentSort(tbody) {
  if (!tbody || tbody.dataset.sortInitialized) return;
  tbody.dataset.sortInitialized = '1';

  let dragging = null;

  tbody.addEventListener('dragstart', (event) => {
    const row = event.target.closest('tr');
    if (!event.target.closest('.drag-handle') || !row) {
      event.preventDefault();
      return;
    }
    dragging = row;
    row.classList.add('sortable-dragging');
    event.dataTransfer.effectAllowed = 'move';
    // Firefox requires data to be set for a drag to start.
    event.dataTransfer.setData('text/plain', row.textContent || '');
  });

  tbody.addEventListener('dragover', (event) => {
    if (!dragging) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = 'move';
    const row = event.target.closest('tr');
    if (!row || row === dragging) return;
    const rect = row.getBoundingClientRect();
    const after = event.clientY > rect.top + rect.height / 2;
    tbody.insertBefore(dragging, after ? row.nextSibling : row);
  });

  const finish = () => {
    if (!dragging) return;
    dragging.classList.remove('sortable-dragging');
    dragging = null;
    tbody.dispatchEvent(new CustomEvent('end', { bubbles: true }));
  };

  tbody.addEventListener('drop', (event) => {
    event.preventDefault();
    finish();
  });
  tbody.addEventListener('dragend', finish);
}