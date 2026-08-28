.global _start
.global load_ethereal
.global ethereal_after
.type _start, %function
_start:
	mov x20, sp
	stp x0, x1, [sp, #-16]!
	stp x2, x3, [sp, #-16]!
	stp x4, x19, [sp, #-16]!
	bl load_ethereal
	ldp x4, x19, [sp], #16
	ldp x2, x3, [sp], #16
	ldp x0, x1, [sp], #16
	mov x0, x20
	bl ethereal_after
	cbz x0, 1f
	br x0
1:
	b 1b
.size _start, .-_start
