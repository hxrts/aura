// Lean compiler output
// Module: Aura.Runner
// Imports: Init Lean.Data.Json Aura
#include <lean/lean.h>
#if defined(__clang__)
#pragma clang diagnostic ignored "-Wunused-parameter"
#pragma clang diagnostic ignored "-Wunused-label"
#elif defined(__GNUC__) && !defined(__CLANG__)
#pragma GCC diagnostic ignored "-Wunused-parameter"
#pragma GCC diagnostic ignored "-Wunused-label"
#pragma GCC diagnostic ignored "-Wunused-but-set-variable"
#endif
#ifdef __cplusplus
extern "C" {
#endif
LEAN_EXPORT lean_object* l_IO_println___at___Aura_Runner_runCommand_spec__0(lean_object*, lean_object*);
static lean_object* l_Aura_Runner_runCommand___closed__10;
LEAN_EXPORT lean_object* l_IO_print___at___IO_println___at___Aura_Runner_runCommand_spec__0_spec__0(lean_object*, lean_object*);
LEAN_EXPORT lean_object* l_Aura_Runner_runCommand___boxed(lean_object*, lean_object*);
static lean_object* l_Aura_Runner_runCommand___closed__7;
LEAN_EXPORT lean_object* l_Aura_Runner_runCommand(lean_object*, lean_object*);
static lean_object* l_Aura_Runner_runCommand___closed__6;
uint8_t lean_string_dec_eq(lean_object*, lean_object*);
lean_object* lean_string_push(lean_object*, uint32_t);
static lean_object* l_Aura_Runner_runCommand___closed__11;
lean_object* lean_get_stdout(lean_object*);
LEAN_EXPORT lean_object* l_Aura_Runner_main(lean_object*, lean_object*);
static lean_object* l_Aura_Runner_runCommand___closed__2;
static lean_object* l_Aura_Runner_runCommand___closed__8;
LEAN_EXPORT lean_object* l_Aura_Runner_main___boxed(lean_object*, lean_object*);
static lean_object* l_Aura_Runner_runCommand___closed__1;
static lean_object* l_Aura_Runner_runCommand___closed__13;
static lean_object* l_Aura_Runner_runCommand___closed__16;
static lean_object* l_Aura_Runner_runCommand___closed__4;
static lean_object* l_Aura_Runner_runCommand___closed__14;
static lean_object* l_Aura_Runner_runCommand___closed__3;
static lean_object* l_Aura_Runner_runCommand___closed__12;
static lean_object* l_Aura_Runner_runCommand___closed__9;
static lean_object* l_Aura_Runner_runCommand___closed__0;
static lean_object* l_Aura_Runner_runCommand___closed__5;
LEAN_EXPORT lean_object* l_Aura_Runner_main___boxed__const__1;
static lean_object* l_Aura_Runner_runCommand___closed__15;
LEAN_EXPORT lean_object* l_IO_print___at___IO_println___at___Aura_Runner_runCommand_spec__0_spec__0(lean_object* x_1, lean_object* x_2) {
_start:
{
lean_object* x_3; lean_object* x_4; lean_object* x_5; lean_object* x_6; lean_object* x_7; 
x_3 = lean_get_stdout(x_2);
x_4 = lean_ctor_get(x_3, 0);
lean_inc(x_4);
x_5 = lean_ctor_get(x_3, 1);
lean_inc(x_5);
lean_dec_ref(x_3);
x_6 = lean_ctor_get(x_4, 4);
lean_inc_ref(x_6);
lean_dec(x_4);
x_7 = lean_apply_2(x_6, x_1, x_5);
return x_7;
}
}
LEAN_EXPORT lean_object* l_IO_println___at___Aura_Runner_runCommand_spec__0(lean_object* x_1, lean_object* x_2) {
_start:
{
uint32_t x_3; lean_object* x_4; lean_object* x_5; 
x_3 = 10;
x_4 = lean_string_push(x_1, x_3);
x_5 = l_IO_print___at___IO_println___at___Aura_Runner_runCommand_spec__0_spec__0(x_4, x_2);
return x_5;
}
}
static lean_object* _init_l_Aura_Runner_runCommand___closed__0() {
_start:
{
lean_object* x_1; 
x_1 = lean_mk_string_unchecked("Usage: aura_verifier <command>", 30, 30);
return x_1;
}
}
static lean_object* _init_l_Aura_Runner_runCommand___closed__1() {
_start:
{
lean_object* x_1; 
x_1 = lean_mk_string_unchecked("Commands:", 9, 9);
return x_1;
}
}
static lean_object* _init_l_Aura_Runner_runCommand___closed__2() {
_start:
{
lean_object* x_1; 
x_1 = lean_mk_string_unchecked("  version          - Show version", 33, 33);
return x_1;
}
}
static lean_object* _init_l_Aura_Runner_runCommand___closed__3() {
_start:
{
lean_object* x_1; 
x_1 = lean_mk_string_unchecked("  journal-merge    - Verify journal merge", 41, 41);
return x_1;
}
}
static lean_object* _init_l_Aura_Runner_runCommand___closed__4() {
_start:
{
lean_object* x_1; 
x_1 = lean_mk_string_unchecked("  journal-reduce   - Verify journal reduction", 45, 45);
return x_1;
}
}
static lean_object* _init_l_Aura_Runner_runCommand___closed__5() {
_start:
{
lean_object* x_1; 
x_1 = lean_mk_string_unchecked("  guard-evaluate   - Verify guard evaluation", 44, 44);
return x_1;
}
}
static lean_object* _init_l_Aura_Runner_runCommand___closed__6() {
_start:
{
lean_object* x_1; 
x_1 = lean_mk_string_unchecked("  frost-check      - Verify FROST protocol", 42, 42);
return x_1;
}
}
static lean_object* _init_l_Aura_Runner_runCommand___closed__7() {
_start:
{
lean_object* x_1; 
x_1 = lean_mk_string_unchecked("version", 7, 7);
return x_1;
}
}
static lean_object* _init_l_Aura_Runner_runCommand___closed__8() {
_start:
{
lean_object* x_1; 
x_1 = lean_mk_string_unchecked("journal-merge", 13, 13);
return x_1;
}
}
static lean_object* _init_l_Aura_Runner_runCommand___closed__9() {
_start:
{
lean_object* x_1; 
x_1 = lean_mk_string_unchecked("journal-reduce", 14, 14);
return x_1;
}
}
static lean_object* _init_l_Aura_Runner_runCommand___closed__10() {
_start:
{
lean_object* x_1; 
x_1 = lean_mk_string_unchecked("guard-evaluate", 14, 14);
return x_1;
}
}
static lean_object* _init_l_Aura_Runner_runCommand___closed__11() {
_start:
{
lean_object* x_1; 
x_1 = lean_mk_string_unchecked("frost-check", 11, 11);
return x_1;
}
}
static lean_object* _init_l_Aura_Runner_runCommand___closed__12() {
_start:
{
lean_object* x_1; 
x_1 = lean_mk_string_unchecked("FROST state machine verification (not yet implemented)", 54, 54);
return x_1;
}
}
static lean_object* _init_l_Aura_Runner_runCommand___closed__13() {
_start:
{
lean_object* x_1; 
x_1 = lean_mk_string_unchecked("Guard chain evaluation verification (not yet implemented)", 57, 57);
return x_1;
}
}
static lean_object* _init_l_Aura_Runner_runCommand___closed__14() {
_start:
{
lean_object* x_1; 
x_1 = lean_mk_string_unchecked("Journal reduce verification (not yet implemented)", 49, 49);
return x_1;
}
}
static lean_object* _init_l_Aura_Runner_runCommand___closed__15() {
_start:
{
lean_object* x_1; 
x_1 = lean_mk_string_unchecked("Journal merge verification (not yet implemented)", 48, 48);
return x_1;
}
}
static lean_object* _init_l_Aura_Runner_runCommand___closed__16() {
_start:
{
lean_object* x_1; 
x_1 = lean_mk_string_unchecked("Aura Lean Verifier v0.1.0", 25, 25);
return x_1;
}
}
LEAN_EXPORT lean_object* l_Aura_Runner_runCommand(lean_object* x_1, lean_object* x_2) {
_start:
{
lean_object* x_3; 
if (lean_obj_tag(x_1) == 0)
{
x_3 = x_2;
goto block_24;
}
else
{
lean_object* x_25; lean_object* x_26; lean_object* x_27; uint8_t x_28; 
x_25 = lean_ctor_get(x_1, 0);
x_26 = lean_ctor_get(x_1, 1);
x_27 = l_Aura_Runner_runCommand___closed__7;
x_28 = lean_string_dec_eq(x_25, x_27);
if (x_28 == 0)
{
lean_object* x_29; uint8_t x_30; 
x_29 = l_Aura_Runner_runCommand___closed__8;
x_30 = lean_string_dec_eq(x_25, x_29);
if (x_30 == 0)
{
lean_object* x_31; uint8_t x_32; 
x_31 = l_Aura_Runner_runCommand___closed__9;
x_32 = lean_string_dec_eq(x_25, x_31);
if (x_32 == 0)
{
lean_object* x_33; uint8_t x_34; 
x_33 = l_Aura_Runner_runCommand___closed__10;
x_34 = lean_string_dec_eq(x_25, x_33);
if (x_34 == 0)
{
lean_object* x_35; uint8_t x_36; 
x_35 = l_Aura_Runner_runCommand___closed__11;
x_36 = lean_string_dec_eq(x_25, x_35);
if (x_36 == 0)
{
x_3 = x_2;
goto block_24;
}
else
{
if (lean_obj_tag(x_26) == 0)
{
lean_object* x_37; lean_object* x_38; 
x_37 = l_Aura_Runner_runCommand___closed__12;
x_38 = l_IO_println___at___Aura_Runner_runCommand_spec__0(x_37, x_2);
return x_38;
}
else
{
x_3 = x_2;
goto block_24;
}
}
}
else
{
if (lean_obj_tag(x_26) == 0)
{
lean_object* x_39; lean_object* x_40; 
x_39 = l_Aura_Runner_runCommand___closed__13;
x_40 = l_IO_println___at___Aura_Runner_runCommand_spec__0(x_39, x_2);
return x_40;
}
else
{
x_3 = x_2;
goto block_24;
}
}
}
else
{
if (lean_obj_tag(x_26) == 0)
{
lean_object* x_41; lean_object* x_42; 
x_41 = l_Aura_Runner_runCommand___closed__14;
x_42 = l_IO_println___at___Aura_Runner_runCommand_spec__0(x_41, x_2);
return x_42;
}
else
{
x_3 = x_2;
goto block_24;
}
}
}
else
{
if (lean_obj_tag(x_26) == 0)
{
lean_object* x_43; lean_object* x_44; 
x_43 = l_Aura_Runner_runCommand___closed__15;
x_44 = l_IO_println___at___Aura_Runner_runCommand_spec__0(x_43, x_2);
return x_44;
}
else
{
x_3 = x_2;
goto block_24;
}
}
}
else
{
if (lean_obj_tag(x_26) == 0)
{
lean_object* x_45; lean_object* x_46; 
x_45 = l_Aura_Runner_runCommand___closed__16;
x_46 = l_IO_println___at___Aura_Runner_runCommand_spec__0(x_45, x_2);
return x_46;
}
else
{
x_3 = x_2;
goto block_24;
}
}
}
block_24:
{
lean_object* x_4; lean_object* x_5; 
x_4 = l_Aura_Runner_runCommand___closed__0;
x_5 = l_IO_println___at___Aura_Runner_runCommand_spec__0(x_4, x_3);
if (lean_obj_tag(x_5) == 0)
{
lean_object* x_6; lean_object* x_7; lean_object* x_8; 
x_6 = lean_ctor_get(x_5, 1);
lean_inc(x_6);
lean_dec_ref(x_5);
x_7 = l_Aura_Runner_runCommand___closed__1;
x_8 = l_IO_println___at___Aura_Runner_runCommand_spec__0(x_7, x_6);
if (lean_obj_tag(x_8) == 0)
{
lean_object* x_9; lean_object* x_10; lean_object* x_11; 
x_9 = lean_ctor_get(x_8, 1);
lean_inc(x_9);
lean_dec_ref(x_8);
x_10 = l_Aura_Runner_runCommand___closed__2;
x_11 = l_IO_println___at___Aura_Runner_runCommand_spec__0(x_10, x_9);
if (lean_obj_tag(x_11) == 0)
{
lean_object* x_12; lean_object* x_13; lean_object* x_14; 
x_12 = lean_ctor_get(x_11, 1);
lean_inc(x_12);
lean_dec_ref(x_11);
x_13 = l_Aura_Runner_runCommand___closed__3;
x_14 = l_IO_println___at___Aura_Runner_runCommand_spec__0(x_13, x_12);
if (lean_obj_tag(x_14) == 0)
{
lean_object* x_15; lean_object* x_16; lean_object* x_17; 
x_15 = lean_ctor_get(x_14, 1);
lean_inc(x_15);
lean_dec_ref(x_14);
x_16 = l_Aura_Runner_runCommand___closed__4;
x_17 = l_IO_println___at___Aura_Runner_runCommand_spec__0(x_16, x_15);
if (lean_obj_tag(x_17) == 0)
{
lean_object* x_18; lean_object* x_19; lean_object* x_20; 
x_18 = lean_ctor_get(x_17, 1);
lean_inc(x_18);
lean_dec_ref(x_17);
x_19 = l_Aura_Runner_runCommand___closed__5;
x_20 = l_IO_println___at___Aura_Runner_runCommand_spec__0(x_19, x_18);
if (lean_obj_tag(x_20) == 0)
{
lean_object* x_21; lean_object* x_22; lean_object* x_23; 
x_21 = lean_ctor_get(x_20, 1);
lean_inc(x_21);
lean_dec_ref(x_20);
x_22 = l_Aura_Runner_runCommand___closed__6;
x_23 = l_IO_println___at___Aura_Runner_runCommand_spec__0(x_22, x_21);
return x_23;
}
else
{
return x_20;
}
}
else
{
return x_17;
}
}
else
{
return x_14;
}
}
else
{
return x_11;
}
}
else
{
return x_8;
}
}
else
{
return x_5;
}
}
}
}
LEAN_EXPORT lean_object* l_Aura_Runner_runCommand___boxed(lean_object* x_1, lean_object* x_2) {
_start:
{
lean_object* x_3; 
x_3 = l_Aura_Runner_runCommand(x_1, x_2);
lean_dec(x_1);
return x_3;
}
}
static lean_object* _init_l_Aura_Runner_main___boxed__const__1() {
_start:
{
uint32_t x_1; lean_object* x_2; 
x_1 = 0;
x_2 = lean_box_uint32(x_1);
return x_2;
}
}
LEAN_EXPORT lean_object* l_Aura_Runner_main(lean_object* x_1, lean_object* x_2) {
_start:
{
lean_object* x_3; 
x_3 = l_Aura_Runner_runCommand(x_1, x_2);
if (lean_obj_tag(x_3) == 0)
{
uint8_t x_4; 
x_4 = !lean_is_exclusive(x_3);
if (x_4 == 0)
{
lean_object* x_5; lean_object* x_6; 
x_5 = lean_ctor_get(x_3, 0);
lean_dec(x_5);
x_6 = l_Aura_Runner_main___boxed__const__1;
lean_ctor_set(x_3, 0, x_6);
return x_3;
}
else
{
lean_object* x_7; lean_object* x_8; lean_object* x_9; 
x_7 = lean_ctor_get(x_3, 1);
lean_inc(x_7);
lean_dec(x_3);
x_8 = l_Aura_Runner_main___boxed__const__1;
x_9 = lean_alloc_ctor(0, 2, 0);
lean_ctor_set(x_9, 0, x_8);
lean_ctor_set(x_9, 1, x_7);
return x_9;
}
}
else
{
uint8_t x_10; 
x_10 = !lean_is_exclusive(x_3);
if (x_10 == 0)
{
return x_3;
}
else
{
lean_object* x_11; lean_object* x_12; lean_object* x_13; 
x_11 = lean_ctor_get(x_3, 0);
x_12 = lean_ctor_get(x_3, 1);
lean_inc(x_12);
lean_inc(x_11);
lean_dec(x_3);
x_13 = lean_alloc_ctor(1, 2, 0);
lean_ctor_set(x_13, 0, x_11);
lean_ctor_set(x_13, 1, x_12);
return x_13;
}
}
}
}
LEAN_EXPORT lean_object* l_Aura_Runner_main___boxed(lean_object* x_1, lean_object* x_2) {
_start:
{
lean_object* x_3; 
x_3 = l_Aura_Runner_main(x_1, x_2);
lean_dec(x_1);
return x_3;
}
}
lean_object* initialize_Init(uint8_t builtin, lean_object*);
lean_object* initialize_Lean_Data_Json(uint8_t builtin, lean_object*);
lean_object* initialize_Aura(uint8_t builtin, lean_object*);
static bool _G_initialized = false;
LEAN_EXPORT lean_object* initialize_Aura_Runner(uint8_t builtin, lean_object* w) {
lean_object * res;
if (_G_initialized) return lean_io_result_mk_ok(lean_box(0));
_G_initialized = true;
res = initialize_Init(builtin, lean_io_mk_world());
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
res = initialize_Lean_Data_Json(builtin, lean_io_mk_world());
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
res = initialize_Aura(builtin, lean_io_mk_world());
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
l_Aura_Runner_runCommand___closed__0 = _init_l_Aura_Runner_runCommand___closed__0();
lean_mark_persistent(l_Aura_Runner_runCommand___closed__0);
l_Aura_Runner_runCommand___closed__1 = _init_l_Aura_Runner_runCommand___closed__1();
lean_mark_persistent(l_Aura_Runner_runCommand___closed__1);
l_Aura_Runner_runCommand___closed__2 = _init_l_Aura_Runner_runCommand___closed__2();
lean_mark_persistent(l_Aura_Runner_runCommand___closed__2);
l_Aura_Runner_runCommand___closed__3 = _init_l_Aura_Runner_runCommand___closed__3();
lean_mark_persistent(l_Aura_Runner_runCommand___closed__3);
l_Aura_Runner_runCommand___closed__4 = _init_l_Aura_Runner_runCommand___closed__4();
lean_mark_persistent(l_Aura_Runner_runCommand___closed__4);
l_Aura_Runner_runCommand___closed__5 = _init_l_Aura_Runner_runCommand___closed__5();
lean_mark_persistent(l_Aura_Runner_runCommand___closed__5);
l_Aura_Runner_runCommand___closed__6 = _init_l_Aura_Runner_runCommand___closed__6();
lean_mark_persistent(l_Aura_Runner_runCommand___closed__6);
l_Aura_Runner_runCommand___closed__7 = _init_l_Aura_Runner_runCommand___closed__7();
lean_mark_persistent(l_Aura_Runner_runCommand___closed__7);
l_Aura_Runner_runCommand___closed__8 = _init_l_Aura_Runner_runCommand___closed__8();
lean_mark_persistent(l_Aura_Runner_runCommand___closed__8);
l_Aura_Runner_runCommand___closed__9 = _init_l_Aura_Runner_runCommand___closed__9();
lean_mark_persistent(l_Aura_Runner_runCommand___closed__9);
l_Aura_Runner_runCommand___closed__10 = _init_l_Aura_Runner_runCommand___closed__10();
lean_mark_persistent(l_Aura_Runner_runCommand___closed__10);
l_Aura_Runner_runCommand___closed__11 = _init_l_Aura_Runner_runCommand___closed__11();
lean_mark_persistent(l_Aura_Runner_runCommand___closed__11);
l_Aura_Runner_runCommand___closed__12 = _init_l_Aura_Runner_runCommand___closed__12();
lean_mark_persistent(l_Aura_Runner_runCommand___closed__12);
l_Aura_Runner_runCommand___closed__13 = _init_l_Aura_Runner_runCommand___closed__13();
lean_mark_persistent(l_Aura_Runner_runCommand___closed__13);
l_Aura_Runner_runCommand___closed__14 = _init_l_Aura_Runner_runCommand___closed__14();
lean_mark_persistent(l_Aura_Runner_runCommand___closed__14);
l_Aura_Runner_runCommand___closed__15 = _init_l_Aura_Runner_runCommand___closed__15();
lean_mark_persistent(l_Aura_Runner_runCommand___closed__15);
l_Aura_Runner_runCommand___closed__16 = _init_l_Aura_Runner_runCommand___closed__16();
lean_mark_persistent(l_Aura_Runner_runCommand___closed__16);
l_Aura_Runner_main___boxed__const__1 = _init_l_Aura_Runner_main___boxed__const__1();
lean_mark_persistent(l_Aura_Runner_main___boxed__const__1);
return lean_io_result_mk_ok(lean_box(0));
}
#ifdef __cplusplus
}
#endif
