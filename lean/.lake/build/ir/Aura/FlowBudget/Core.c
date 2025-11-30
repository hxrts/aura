// Lean compiler output
// Module: Aura.FlowBudget.Core
// Imports: Init
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
LEAN_EXPORT lean_object* l_Aura_FlowBudget_charge___boxed(lean_object*, lean_object*);
static lean_object* l_Aura_FlowBudget_reprBudget___redArg___closed__4____x40_Aura_FlowBudget_Core___hyg_76_;
LEAN_EXPORT lean_object* l_Aura_FlowBudget_reprBudget____x40_Aura_FlowBudget_Core___hyg_76_(lean_object*, lean_object*);
static lean_object* l_Aura_FlowBudget_reprBudget___redArg___closed__9____x40_Aura_FlowBudget_Core___hyg_76_;
static lean_object* l_Aura_FlowBudget_reprBudget___redArg___closed__6____x40_Aura_FlowBudget_Core___hyg_76_;
LEAN_EXPORT uint8_t l_Aura_FlowBudget_beqBudget____x40_Aura_FlowBudget_Core___hyg_18_(lean_object*, lean_object*);
static lean_object* l_Aura_FlowBudget_instBEqBudget___closed__0;
static lean_object* l_Aura_FlowBudget_reprBudget___redArg___closed__0____x40_Aura_FlowBudget_Core___hyg_76_;
static lean_object* l_Aura_FlowBudget_reprBudget___redArg___closed__8____x40_Aura_FlowBudget_Core___hyg_76_;
lean_object* l_Nat_reprFast(lean_object*);
static lean_object* l_Aura_FlowBudget_reprBudget___redArg___closed__11____x40_Aura_FlowBudget_Core___hyg_76_;
lean_object* lean_nat_to_int(lean_object*);
static lean_object* l_Aura_FlowBudget_reprBudget___redArg___closed__1____x40_Aura_FlowBudget_Core___hyg_76_;
static lean_object* l_Aura_FlowBudget_reprBudget___redArg___closed__7____x40_Aura_FlowBudget_Core___hyg_76_;
static lean_object* l_Aura_FlowBudget_reprBudget___redArg___closed__3____x40_Aura_FlowBudget_Core___hyg_76_;
LEAN_EXPORT lean_object* l_Aura_FlowBudget_reprBudget___redArg____x40_Aura_FlowBudget_Core___hyg_76_(lean_object*);
static lean_object* l_Aura_FlowBudget_reprBudget___redArg___closed__5____x40_Aura_FlowBudget_Core___hyg_76_;
static lean_object* l_Aura_FlowBudget_reprBudget___redArg___closed__2____x40_Aura_FlowBudget_Core___hyg_76_;
LEAN_EXPORT lean_object* l_Aura_FlowBudget_charge(lean_object*, lean_object*);
static lean_object* l_Aura_FlowBudget_reprBudget___redArg___closed__10____x40_Aura_FlowBudget_Core___hyg_76_;
LEAN_EXPORT lean_object* l_Aura_FlowBudget_beqBudget____x40_Aura_FlowBudget_Core___hyg_18____boxed(lean_object*, lean_object*);
LEAN_EXPORT lean_object* l_Aura_FlowBudget_instBEqBudget;
uint8_t lean_nat_dec_eq(lean_object*, lean_object*);
lean_object* lean_nat_sub(lean_object*, lean_object*);
LEAN_EXPORT lean_object* l_Aura_FlowBudget_reprBudget____x40_Aura_FlowBudget_Core___hyg_76____boxed(lean_object*, lean_object*);
uint8_t lean_nat_dec_le(lean_object*, lean_object*);
LEAN_EXPORT lean_object* l_Aura_FlowBudget_instReprBudget;
static lean_object* l_Aura_FlowBudget_instReprBudget___closed__0;
LEAN_EXPORT uint8_t l_Aura_FlowBudget_beqBudget____x40_Aura_FlowBudget_Core___hyg_18_(lean_object* x_1, lean_object* x_2) {
_start:
{
uint8_t x_3; 
x_3 = lean_nat_dec_eq(x_1, x_2);
return x_3;
}
}
LEAN_EXPORT lean_object* l_Aura_FlowBudget_beqBudget____x40_Aura_FlowBudget_Core___hyg_18____boxed(lean_object* x_1, lean_object* x_2) {
_start:
{
uint8_t x_3; lean_object* x_4; 
x_3 = l_Aura_FlowBudget_beqBudget____x40_Aura_FlowBudget_Core___hyg_18_(x_1, x_2);
lean_dec(x_2);
lean_dec(x_1);
x_4 = lean_box(x_3);
return x_4;
}
}
static lean_object* _init_l_Aura_FlowBudget_instBEqBudget___closed__0() {
_start:
{
lean_object* x_1; 
x_1 = lean_alloc_closure((void*)(l_Aura_FlowBudget_beqBudget____x40_Aura_FlowBudget_Core___hyg_18____boxed), 2, 0);
return x_1;
}
}
static lean_object* _init_l_Aura_FlowBudget_instBEqBudget() {
_start:
{
lean_object* x_1; 
x_1 = l_Aura_FlowBudget_instBEqBudget___closed__0;
return x_1;
}
}
static lean_object* _init_l_Aura_FlowBudget_reprBudget___redArg___closed__0____x40_Aura_FlowBudget_Core___hyg_76_() {
_start:
{
lean_object* x_1; 
x_1 = lean_mk_string_unchecked("{ ", 2, 2);
return x_1;
}
}
static lean_object* _init_l_Aura_FlowBudget_reprBudget___redArg___closed__1____x40_Aura_FlowBudget_Core___hyg_76_() {
_start:
{
lean_object* x_1; 
x_1 = lean_mk_string_unchecked("available", 9, 9);
return x_1;
}
}
static lean_object* _init_l_Aura_FlowBudget_reprBudget___redArg___closed__2____x40_Aura_FlowBudget_Core___hyg_76_() {
_start:
{
lean_object* x_1; lean_object* x_2; 
x_1 = l_Aura_FlowBudget_reprBudget___redArg___closed__1____x40_Aura_FlowBudget_Core___hyg_76_;
x_2 = lean_alloc_ctor(3, 1, 0);
lean_ctor_set(x_2, 0, x_1);
return x_2;
}
}
static lean_object* _init_l_Aura_FlowBudget_reprBudget___redArg___closed__3____x40_Aura_FlowBudget_Core___hyg_76_() {
_start:
{
lean_object* x_1; lean_object* x_2; lean_object* x_3; 
x_1 = l_Aura_FlowBudget_reprBudget___redArg___closed__2____x40_Aura_FlowBudget_Core___hyg_76_;
x_2 = lean_box(0);
x_3 = lean_alloc_ctor(5, 2, 0);
lean_ctor_set(x_3, 0, x_2);
lean_ctor_set(x_3, 1, x_1);
return x_3;
}
}
static lean_object* _init_l_Aura_FlowBudget_reprBudget___redArg___closed__4____x40_Aura_FlowBudget_Core___hyg_76_() {
_start:
{
lean_object* x_1; 
x_1 = lean_mk_string_unchecked(" := ", 4, 4);
return x_1;
}
}
static lean_object* _init_l_Aura_FlowBudget_reprBudget___redArg___closed__5____x40_Aura_FlowBudget_Core___hyg_76_() {
_start:
{
lean_object* x_1; lean_object* x_2; 
x_1 = l_Aura_FlowBudget_reprBudget___redArg___closed__4____x40_Aura_FlowBudget_Core___hyg_76_;
x_2 = lean_alloc_ctor(3, 1, 0);
lean_ctor_set(x_2, 0, x_1);
return x_2;
}
}
static lean_object* _init_l_Aura_FlowBudget_reprBudget___redArg___closed__6____x40_Aura_FlowBudget_Core___hyg_76_() {
_start:
{
lean_object* x_1; lean_object* x_2; lean_object* x_3; 
x_1 = l_Aura_FlowBudget_reprBudget___redArg___closed__5____x40_Aura_FlowBudget_Core___hyg_76_;
x_2 = l_Aura_FlowBudget_reprBudget___redArg___closed__3____x40_Aura_FlowBudget_Core___hyg_76_;
x_3 = lean_alloc_ctor(5, 2, 0);
lean_ctor_set(x_3, 0, x_2);
lean_ctor_set(x_3, 1, x_1);
return x_3;
}
}
static lean_object* _init_l_Aura_FlowBudget_reprBudget___redArg___closed__7____x40_Aura_FlowBudget_Core___hyg_76_() {
_start:
{
lean_object* x_1; lean_object* x_2; 
x_1 = lean_unsigned_to_nat(13u);
x_2 = lean_nat_to_int(x_1);
return x_2;
}
}
static lean_object* _init_l_Aura_FlowBudget_reprBudget___redArg___closed__8____x40_Aura_FlowBudget_Core___hyg_76_() {
_start:
{
lean_object* x_1; 
x_1 = lean_mk_string_unchecked(" }", 2, 2);
return x_1;
}
}
static lean_object* _init_l_Aura_FlowBudget_reprBudget___redArg___closed__9____x40_Aura_FlowBudget_Core___hyg_76_() {
_start:
{
lean_object* x_1; lean_object* x_2; 
x_1 = lean_unsigned_to_nat(2u);
x_2 = lean_nat_to_int(x_1);
return x_2;
}
}
static lean_object* _init_l_Aura_FlowBudget_reprBudget___redArg___closed__10____x40_Aura_FlowBudget_Core___hyg_76_() {
_start:
{
lean_object* x_1; lean_object* x_2; 
x_1 = l_Aura_FlowBudget_reprBudget___redArg___closed__0____x40_Aura_FlowBudget_Core___hyg_76_;
x_2 = lean_alloc_ctor(3, 1, 0);
lean_ctor_set(x_2, 0, x_1);
return x_2;
}
}
static lean_object* _init_l_Aura_FlowBudget_reprBudget___redArg___closed__11____x40_Aura_FlowBudget_Core___hyg_76_() {
_start:
{
lean_object* x_1; lean_object* x_2; 
x_1 = l_Aura_FlowBudget_reprBudget___redArg___closed__8____x40_Aura_FlowBudget_Core___hyg_76_;
x_2 = lean_alloc_ctor(3, 1, 0);
lean_ctor_set(x_2, 0, x_1);
return x_2;
}
}
LEAN_EXPORT lean_object* l_Aura_FlowBudget_reprBudget___redArg____x40_Aura_FlowBudget_Core___hyg_76_(lean_object* x_1) {
_start:
{
lean_object* x_2; lean_object* x_3; lean_object* x_4; lean_object* x_5; lean_object* x_6; uint8_t x_7; lean_object* x_8; lean_object* x_9; lean_object* x_10; lean_object* x_11; lean_object* x_12; lean_object* x_13; lean_object* x_14; lean_object* x_15; lean_object* x_16; 
x_2 = l_Aura_FlowBudget_reprBudget___redArg___closed__6____x40_Aura_FlowBudget_Core___hyg_76_;
x_3 = l_Aura_FlowBudget_reprBudget___redArg___closed__7____x40_Aura_FlowBudget_Core___hyg_76_;
x_4 = l_Nat_reprFast(x_1);
x_5 = lean_alloc_ctor(3, 1, 0);
lean_ctor_set(x_5, 0, x_4);
x_6 = lean_alloc_ctor(4, 2, 0);
lean_ctor_set(x_6, 0, x_3);
lean_ctor_set(x_6, 1, x_5);
x_7 = 0;
x_8 = lean_alloc_ctor(6, 1, 1);
lean_ctor_set(x_8, 0, x_6);
lean_ctor_set_uint8(x_8, sizeof(void*)*1, x_7);
x_9 = lean_alloc_ctor(5, 2, 0);
lean_ctor_set(x_9, 0, x_2);
lean_ctor_set(x_9, 1, x_8);
x_10 = l_Aura_FlowBudget_reprBudget___redArg___closed__9____x40_Aura_FlowBudget_Core___hyg_76_;
x_11 = l_Aura_FlowBudget_reprBudget___redArg___closed__10____x40_Aura_FlowBudget_Core___hyg_76_;
x_12 = lean_alloc_ctor(5, 2, 0);
lean_ctor_set(x_12, 0, x_11);
lean_ctor_set(x_12, 1, x_9);
x_13 = l_Aura_FlowBudget_reprBudget___redArg___closed__11____x40_Aura_FlowBudget_Core___hyg_76_;
x_14 = lean_alloc_ctor(5, 2, 0);
lean_ctor_set(x_14, 0, x_12);
lean_ctor_set(x_14, 1, x_13);
x_15 = lean_alloc_ctor(4, 2, 0);
lean_ctor_set(x_15, 0, x_10);
lean_ctor_set(x_15, 1, x_14);
x_16 = lean_alloc_ctor(6, 1, 1);
lean_ctor_set(x_16, 0, x_15);
lean_ctor_set_uint8(x_16, sizeof(void*)*1, x_7);
return x_16;
}
}
LEAN_EXPORT lean_object* l_Aura_FlowBudget_reprBudget____x40_Aura_FlowBudget_Core___hyg_76_(lean_object* x_1, lean_object* x_2) {
_start:
{
lean_object* x_3; 
x_3 = l_Aura_FlowBudget_reprBudget___redArg____x40_Aura_FlowBudget_Core___hyg_76_(x_1);
return x_3;
}
}
LEAN_EXPORT lean_object* l_Aura_FlowBudget_reprBudget____x40_Aura_FlowBudget_Core___hyg_76____boxed(lean_object* x_1, lean_object* x_2) {
_start:
{
lean_object* x_3; 
x_3 = l_Aura_FlowBudget_reprBudget____x40_Aura_FlowBudget_Core___hyg_76_(x_1, x_2);
lean_dec(x_2);
return x_3;
}
}
static lean_object* _init_l_Aura_FlowBudget_instReprBudget___closed__0() {
_start:
{
lean_object* x_1; 
x_1 = lean_alloc_closure((void*)(l_Aura_FlowBudget_reprBudget____x40_Aura_FlowBudget_Core___hyg_76____boxed), 2, 0);
return x_1;
}
}
static lean_object* _init_l_Aura_FlowBudget_instReprBudget() {
_start:
{
lean_object* x_1; 
x_1 = l_Aura_FlowBudget_instReprBudget___closed__0;
return x_1;
}
}
LEAN_EXPORT lean_object* l_Aura_FlowBudget_charge(lean_object* x_1, lean_object* x_2) {
_start:
{
uint8_t x_3; 
x_3 = lean_nat_dec_le(x_2, x_1);
if (x_3 == 0)
{
lean_object* x_4; 
x_4 = lean_box(0);
return x_4;
}
else
{
lean_object* x_5; lean_object* x_6; 
x_5 = lean_nat_sub(x_1, x_2);
x_6 = lean_alloc_ctor(1, 1, 0);
lean_ctor_set(x_6, 0, x_5);
return x_6;
}
}
}
LEAN_EXPORT lean_object* l_Aura_FlowBudget_charge___boxed(lean_object* x_1, lean_object* x_2) {
_start:
{
lean_object* x_3; 
x_3 = l_Aura_FlowBudget_charge(x_1, x_2);
lean_dec(x_2);
lean_dec(x_1);
return x_3;
}
}
lean_object* initialize_Init(uint8_t builtin, lean_object*);
static bool _G_initialized = false;
LEAN_EXPORT lean_object* initialize_Aura_FlowBudget_Core(uint8_t builtin, lean_object* w) {
lean_object * res;
if (_G_initialized) return lean_io_result_mk_ok(lean_box(0));
_G_initialized = true;
res = initialize_Init(builtin, lean_io_mk_world());
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
l_Aura_FlowBudget_instBEqBudget___closed__0 = _init_l_Aura_FlowBudget_instBEqBudget___closed__0();
lean_mark_persistent(l_Aura_FlowBudget_instBEqBudget___closed__0);
l_Aura_FlowBudget_instBEqBudget = _init_l_Aura_FlowBudget_instBEqBudget();
lean_mark_persistent(l_Aura_FlowBudget_instBEqBudget);
l_Aura_FlowBudget_reprBudget___redArg___closed__0____x40_Aura_FlowBudget_Core___hyg_76_ = _init_l_Aura_FlowBudget_reprBudget___redArg___closed__0____x40_Aura_FlowBudget_Core___hyg_76_();
lean_mark_persistent(l_Aura_FlowBudget_reprBudget___redArg___closed__0____x40_Aura_FlowBudget_Core___hyg_76_);
l_Aura_FlowBudget_reprBudget___redArg___closed__1____x40_Aura_FlowBudget_Core___hyg_76_ = _init_l_Aura_FlowBudget_reprBudget___redArg___closed__1____x40_Aura_FlowBudget_Core___hyg_76_();
lean_mark_persistent(l_Aura_FlowBudget_reprBudget___redArg___closed__1____x40_Aura_FlowBudget_Core___hyg_76_);
l_Aura_FlowBudget_reprBudget___redArg___closed__2____x40_Aura_FlowBudget_Core___hyg_76_ = _init_l_Aura_FlowBudget_reprBudget___redArg___closed__2____x40_Aura_FlowBudget_Core___hyg_76_();
lean_mark_persistent(l_Aura_FlowBudget_reprBudget___redArg___closed__2____x40_Aura_FlowBudget_Core___hyg_76_);
l_Aura_FlowBudget_reprBudget___redArg___closed__3____x40_Aura_FlowBudget_Core___hyg_76_ = _init_l_Aura_FlowBudget_reprBudget___redArg___closed__3____x40_Aura_FlowBudget_Core___hyg_76_();
lean_mark_persistent(l_Aura_FlowBudget_reprBudget___redArg___closed__3____x40_Aura_FlowBudget_Core___hyg_76_);
l_Aura_FlowBudget_reprBudget___redArg___closed__4____x40_Aura_FlowBudget_Core___hyg_76_ = _init_l_Aura_FlowBudget_reprBudget___redArg___closed__4____x40_Aura_FlowBudget_Core___hyg_76_();
lean_mark_persistent(l_Aura_FlowBudget_reprBudget___redArg___closed__4____x40_Aura_FlowBudget_Core___hyg_76_);
l_Aura_FlowBudget_reprBudget___redArg___closed__5____x40_Aura_FlowBudget_Core___hyg_76_ = _init_l_Aura_FlowBudget_reprBudget___redArg___closed__5____x40_Aura_FlowBudget_Core___hyg_76_();
lean_mark_persistent(l_Aura_FlowBudget_reprBudget___redArg___closed__5____x40_Aura_FlowBudget_Core___hyg_76_);
l_Aura_FlowBudget_reprBudget___redArg___closed__6____x40_Aura_FlowBudget_Core___hyg_76_ = _init_l_Aura_FlowBudget_reprBudget___redArg___closed__6____x40_Aura_FlowBudget_Core___hyg_76_();
lean_mark_persistent(l_Aura_FlowBudget_reprBudget___redArg___closed__6____x40_Aura_FlowBudget_Core___hyg_76_);
l_Aura_FlowBudget_reprBudget___redArg___closed__7____x40_Aura_FlowBudget_Core___hyg_76_ = _init_l_Aura_FlowBudget_reprBudget___redArg___closed__7____x40_Aura_FlowBudget_Core___hyg_76_();
lean_mark_persistent(l_Aura_FlowBudget_reprBudget___redArg___closed__7____x40_Aura_FlowBudget_Core___hyg_76_);
l_Aura_FlowBudget_reprBudget___redArg___closed__8____x40_Aura_FlowBudget_Core___hyg_76_ = _init_l_Aura_FlowBudget_reprBudget___redArg___closed__8____x40_Aura_FlowBudget_Core___hyg_76_();
lean_mark_persistent(l_Aura_FlowBudget_reprBudget___redArg___closed__8____x40_Aura_FlowBudget_Core___hyg_76_);
l_Aura_FlowBudget_reprBudget___redArg___closed__9____x40_Aura_FlowBudget_Core___hyg_76_ = _init_l_Aura_FlowBudget_reprBudget___redArg___closed__9____x40_Aura_FlowBudget_Core___hyg_76_();
lean_mark_persistent(l_Aura_FlowBudget_reprBudget___redArg___closed__9____x40_Aura_FlowBudget_Core___hyg_76_);
l_Aura_FlowBudget_reprBudget___redArg___closed__10____x40_Aura_FlowBudget_Core___hyg_76_ = _init_l_Aura_FlowBudget_reprBudget___redArg___closed__10____x40_Aura_FlowBudget_Core___hyg_76_();
lean_mark_persistent(l_Aura_FlowBudget_reprBudget___redArg___closed__10____x40_Aura_FlowBudget_Core___hyg_76_);
l_Aura_FlowBudget_reprBudget___redArg___closed__11____x40_Aura_FlowBudget_Core___hyg_76_ = _init_l_Aura_FlowBudget_reprBudget___redArg___closed__11____x40_Aura_FlowBudget_Core___hyg_76_();
lean_mark_persistent(l_Aura_FlowBudget_reprBudget___redArg___closed__11____x40_Aura_FlowBudget_Core___hyg_76_);
l_Aura_FlowBudget_instReprBudget___closed__0 = _init_l_Aura_FlowBudget_instReprBudget___closed__0();
lean_mark_persistent(l_Aura_FlowBudget_instReprBudget___closed__0);
l_Aura_FlowBudget_instReprBudget = _init_l_Aura_FlowBudget_instReprBudget();
lean_mark_persistent(l_Aura_FlowBudget_instReprBudget);
return lean_io_result_mk_ok(lean_box(0));
}
#ifdef __cplusplus
}
#endif
