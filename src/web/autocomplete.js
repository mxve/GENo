function initAutocomplete(inputId, dropdownId, onSelect) {
  const input = document.getElementById(inputId);
  const dropdown = document.getElementById(dropdownId);
  if (!input || !dropdown) return;

  let debounceTimer = null;

  input.addEventListener('input', function() {
    clearTimeout(debounceTimer);
    const val = input.value.trim();
    if (val.length < 2) {
      dropdown.innerHTML = '';
      dropdown.style.display = 'none';
      return;
    }

    debounceTimer = setTimeout(function() {
      fetch('/search?q=' + encodeURIComponent(val))
        .then(function(res) { return res.json(); })
        .then(function(items) {
          dropdown.innerHTML = '';
          if (!items || items.length === 0) {
            dropdown.innerHTML = '<div class="autocomplete-item" style="color:var(--text-dim); cursor:default;">No matching OSRS items</div>';
            dropdown.style.display = 'block';
            return;
          }

          items.forEach(function(item) {
            const div = document.createElement('div');
            div.className = 'autocomplete-item';
            var iconHtml = item.icon ? '<img src="/icons/' + item.id + '" class="item-icon" alt="">' : '';
            div.innerHTML = iconHtml + '<strong>' + escapeHtml(item.name) + '</strong> <span style="color:var(--text-dim); float:right;">#' + item.id + '</span>';

            div.addEventListener('click', function() {
              onSelect(item);
              dropdown.innerHTML = '';
              dropdown.style.display = 'none';
            });
            dropdown.appendChild(div);
          });
          dropdown.style.display = 'block';
        })
        .catch(function(err) {
          console.error('Search error:', err);
        });
    }, 300);
  });

  document.addEventListener('click', function(e) {
    if (!input.contains(e.target) && !dropdown.contains(e.target)) {
      dropdown.style.display = 'none';
    }
  });
}

function escapeHtml(str) {
  return str.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;").replace(/'/g, "&#039;");
}
