(function () {
    'use strict';

    function findContent() {
        return document.querySelector('.content main');
    }

    function buildMarkdown(main) {
        var title = document.title.replace(/\s*[-–]\s*Kael Documentation\s*$/, '');
        var text = '# ' + title + '\n\nSource: ' + window.location.href + '\n\n';
        var elements = main.querySelectorAll('h1, h2, h3, h4, p, pre, ul, ol, table, blockquote');

        elements.forEach(function (element) {
            var tag = element.tagName;
            if (tag === 'H1') {
                text += '# ' + element.textContent.trim() + '\n\n';
            } else if (/^H[2-4]$/.test(tag)) {
                text += '\n' + '#'.repeat(Number(tag.slice(1))) + ' ' + element.textContent.trim() + '\n\n';
            } else if (tag === 'PRE') {
                var code = element.querySelector('code');
                var match = code && code.className.match(/language-(\w+)/);
                text += '```' + (match ? match[1] : '') + '\n' + element.textContent.trim() + '\n```\n\n';
            } else if (tag === 'TABLE') {
                element.querySelectorAll('tr').forEach(function (row, index) {
                    var cells = row.querySelectorAll('th, td');
                    text += '| ' + Array.from(cells).map(function (cell) {
                        return cell.textContent.trim();
                    }).join(' | ') + ' |\n';
                    if (index === 0) text += '| ' + Array.from(cells).map(function () { return '---'; }).join(' | ') + ' |\n';
                });
                text += '\n';
            } else if (tag === 'UL' || tag === 'OL') {
                Array.from(element.children).forEach(function (item, index) {
                    text += (tag === 'OL' ? (index + 1) + '. ' : '- ') + item.textContent.trim() + '\n';
                });
                text += '\n';
            } else if (tag === 'BLOCKQUOTE') {
                text += '> ' + element.textContent.trim() + '\n\n';
            } else if (!element.closest('li, blockquote, td, th')) {
                text += element.textContent.trim() + '\n\n';
            }
        });

        return text;
    }

    function copyText(text) {
        if (navigator.clipboard && navigator.clipboard.writeText) {
            return navigator.clipboard.writeText(text);
        }

        return new Promise(function (resolve, reject) {
            var textarea = document.createElement('textarea');
            textarea.value = text;
            textarea.setAttribute('readonly', '');
            textarea.style.cssText = 'position:fixed;top:-1000px;opacity:0';
            document.body.appendChild(textarea);
            textarea.select();
            var copied = document.execCommand('copy');
            textarea.remove();
            copied ? resolve() : reject(new Error('Copy failed'));
        });
    }

    function flash(button, label, className) {
        button.textContent = label;
        button.classList.remove('copied', 'copy-failed');
        if (className) button.classList.add(className);
        window.setTimeout(function () {
            button.textContent = 'Copy page';
            button.classList.remove('copied', 'copy-failed');
        }, 1800);
    }

    function addWordmark() {
        var scrollbox = document.querySelector('.sidebar-scrollbox');
        if (!scrollbox || scrollbox.querySelector('.kael-wordmark')) return;

        var mark = document.createElement('a');
        mark.className = 'kael-wordmark';
        mark.href = (window.path_to_root || '') + 'index.html';
        mark.innerHTML = 'Kael <span>Documentation · 0.4</span>';
        scrollbox.prepend(mark);
    }

    function refineChrome() {
        document.body.classList.toggle('is-home', Boolean(document.querySelector('.kael-home')));
        document.body.classList.toggle('is-object-guide', /\/object-guide\.html$/.test(window.location.pathname));

        var title = document.querySelector('.menu-title');
        if (title && !title.querySelector('span')) title.innerHTML = 'Kael <span>Documentation</span>';

        addWordmark();
    }

    function addCopyButton() {
        if (document.querySelector('.copy-llm-btn')) return;
        var host = document.querySelector('#menu-bar .right-buttons');
        if (!host) return;

        var button = document.createElement('button');
        button.className = 'copy-llm-btn';
        button.type = 'button';
        button.textContent = 'Copy page';
        button.title = 'Copy this page as Markdown';
        button.setAttribute('aria-label', 'Copy this page as Markdown');
        button.addEventListener('click', function () {
            var main = findContent();
            if (!main) {
                flash(button, 'Copy failed', 'copy-failed');
                return;
            }
            copyText(buildMarkdown(main)).then(function () {
                flash(button, 'Copied', 'copied');
            }).catch(function () {
                flash(button, 'Copy failed', 'copy-failed');
            });
        });
        host.appendChild(button);
    }

    function enhance() {
        refineChrome();
        addCopyButton();
    }

    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', enhance, { once: true });
    } else {
        enhance();
    }

    new MutationObserver(enhance).observe(document.body, { childList: true, subtree: true });
})();
