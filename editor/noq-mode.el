;;; noq-mode.el --- Major Mode for editing Noq source code -*- lexical-binding: t -*-

;;; Commentary:
;;
;; Major Mode for editing Noq source code

(defconst noq-mode-syntax-table
  (with-syntax-table (copy-syntax-table)
    ;; Python style comments
    (modify-syntax-entry ?# "<")
    (modify-syntax-entry ?\n ">")
	;; (modify-syntax-entry ?/ ". 124b")
	;; (modify-syntax-entry ?* ". 23")
	;; (modify-syntax-entry ?\n "> b")
    ;; Chars are the same as strings
    (modify-syntax-entry ?' "\"")
    (syntax-table))
  "Syntax table for `noq-mode'.")

(eval-and-compile
  (defconst noq-apply-strategies
    '("all" "deep")))

(eval-and-compile
  (defconst noq-keywords
    '("undo" "quit" "delete" "load" "save")))

(defconst noq-highlights
  `((
    ;; Keywords
    ,(regexp-opt noq-keywords 'words) . 'font-lock-keyword-face)

    ;; `Apply` strategies
    (,(format "\\(%s\\)[\t ]*|" (mapconcat 'regexp-quote noq-apply-strategies "\\|"))
     1 'font-lock-type-face)
    ("\\([0-9]+\\)[\t ]*|" 1 'font-lock-type-face)

    ;; Variables
    ("\\(^\\|[^a-zA-Z0-9_]\\)\\([_A-Z][_a-zA-Z0-9]*\\)" 2 'font-lock-variable-name-face)

    ;; Functor names
    ("\\([^\n\| ]*\\)[\t ]*::" 1 'font-lock-function-name-face)
    ))

;;;###autoload
(define-derived-mode noq-mode prog-mode "noq"
  "Major Mode for editing Noq source code."
  :syntax-table noq-mode-syntax-table
  (setq font-lock-defaults '(noq-highlights))
  (setq-local comment-start "# "))

;;;###autoload
(add-to-list 'auto-mode-alist '("\\.noq\\'" . noq-mode))

(provide 'noq-mode)

;;; noq-mode.el ends here
