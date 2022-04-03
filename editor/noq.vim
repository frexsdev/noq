" Vim syntax file
" Language: Noq

" Usage Instructions
" Put this file in .vim/syntax/noq.vim
" and add in your .vimrc file the next line:
" autocmd BufRead,BufNewFile *.noq set filetype=noq

if exists('b:current_syntax')
  finish
endif

" Language keywords
syntax keyword noqKeywords rule shape apply done quit

" Comments
syntax region noqCommentLine start="#" end="$"

" Set highlights
highlight default link noqKeywords Keyword
highlight default link noqCommentLine Comment

let b:current_syntax = 'noq'
