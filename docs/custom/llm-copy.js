(function() {
    'use strict';

    function addCopyLlmButton() {
        if (document.querySelector('.copy-llm-btn')) return;

        var btn = document.createElement('button');
        btn.className = 'copy-llm-btn';
        btn.textContent = 'Copy for LLM';
        btn.title = 'Copy this page as markdown for pasting into an LLM';

        btn.addEventListener('click', function() {
            var content = document.getElementById('content');
            if (!content) return;

            var main = content.querySelector('.content');
            if (!main) main = content;

            var title = document.title.replace(' - Kael Documentation', '');
            var text = '# ' + title + '\n\n';
            text += 'Source: ' + window.location.href + '\n\n';

            var elements = main.querySelectorAll('h1, h2, h3, h4, p, pre, ul, ol, table, blockquote');
            elements.forEach(function(el) {
                if (el.tagName === 'H1') {
                    return;
                } else if (el.tagName === 'H2') {
                    text += '\n## ' + el.textContent.trim() + '\n\n';
                } else if (el.tagName === 'H3') {
                    text += '\n### ' + el.textContent.trim() + '\n\n';
                } else if (el.tagName === 'H4') {
                    text += '\n#### ' + el.textContent.trim() + '\n\n';
                } else if (el.tagName === 'PRE') {
                    var code = el.querySelector('code');
                    var lang = '';
                    if (code && code.className) {
                        var match = code.className.match(/language-(\w+)/);
                        if (match) lang = match[1];
                    }
                    text += '```' + lang + '\n' + el.textContent.trim() + '\n```\n\n';
                } else if (el.tagName === 'TABLE') {
                    var rows = el.querySelectorAll('tr');
                    rows.forEach(function(row, i) {
                        var cells = row.querySelectorAll('th, td');
                        var line = '| ';
                        cells.forEach(function(cell) {
                            line += cell.textContent.trim() + ' | ';
                        });
                        text += line + '\n';
                        if (i === 0) {
                            text += '| ';
                            cells.forEach(function() { text += '--- | '; });
                            text += '\n';
                        }
                    });
                    text += '\n';
                } else if (el.tagName === 'UL' || el.tagName === 'OL') {
                    el.querySelectorAll('li').forEach(function(li, i) {
                        var prefix = el.tagName === 'OL' ? (i + 1) + '. ' : '- ';
                        text += prefix + li.textContent.trim() + '\n';
                    });
                    text += '\n';
                } else if (el.tagName === 'BLOCKQUOTE') {
                    text += '> ' + el.textContent.trim() + '\n\n';
                } else {
                    text += el.textContent.trim() + '\n\n';
                }
            });

            navigator.clipboard.writeText(text).then(function() {
                btn.textContent = 'Copied!';
                btn.classList.add('copied');
                setTimeout(function() {
                    btn.textContent = 'Copy for LLM';
                    btn.classList.remove('copied');
                }, 2000);
            });
        });

        document.body.appendChild(btn);
    }

    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', addCopyLlmButton);
    } else {
        addCopyLlmButton();
    }

    var observer = new MutationObserver(addCopyLlmButton);
    observer.observe(document.body, { childList: true, subtree: true });
})();
