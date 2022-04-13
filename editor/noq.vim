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
syntax keyword noqKeywords all

" Language functions
syntax keyword noqFunctions delete undo delete quit use deep

" String literals
syntax region noqString start=/\v"/ skip=/\v\\./ end=/\v"/

" Number literals
syntax match noqNumber "\v<\d+>"

" Comments
syntax region noqCommentLine start="#" end="$"

" Operators
syntax match noqOperators "+\|-\|*\|\^\|/\|%\|=\|::\|!\|@!"

" Set highlights
highlight default link noqKeywords    Keyword
highlight default link noqFunctions   Function 
highlight default link noqOperators   Keyword
highlight default link noqString      String
highlight default link noqNumber      Number
highlight default link noqCommentLine Comment

let b:current_syntax = 'noq'
