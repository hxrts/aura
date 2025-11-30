// Lean compiler output
// Module: Aura.Journal.Semilattice
// Imports: Init Aura.Journal.Core
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
lean_object* l_Aura_Journal_merge(lean_object*, lean_object*);
lean_object* l_List_eraseDups___at___Aura_Journal_merge_spec__0(lean_object*);
LEAN_EXPORT lean_object* l_Aura_Journal_instJoinSemilatticeJournal;
static lean_object* l_Aura_Journal_instJoinSemilatticeJournal___closed__0;
LEAN_EXPORT lean_object* l_Aura_Journal_reduce(lean_object*);
static lean_object* _init_l_Aura_Journal_instJoinSemilatticeJournal___closed__0() {
_start:
{
lean_object* x_1; 
x_1 = lean_alloc_closure((void*)(l_Aura_Journal_merge), 2, 0);
return x_1;
}
}
static lean_object* _init_l_Aura_Journal_instJoinSemilatticeJournal() {
_start:
{
lean_object* x_1; 
x_1 = l_Aura_Journal_instJoinSemilatticeJournal___closed__0;
return x_1;
}
}
LEAN_EXPORT lean_object* l_Aura_Journal_reduce(lean_object* x_1) {
_start:
{
lean_object* x_2; 
x_2 = l_List_eraseDups___at___Aura_Journal_merge_spec__0(x_1);
return x_2;
}
}
lean_object* initialize_Init(uint8_t builtin, lean_object*);
lean_object* initialize_Aura_Journal_Core(uint8_t builtin, lean_object*);
static bool _G_initialized = false;
LEAN_EXPORT lean_object* initialize_Aura_Journal_Semilattice(uint8_t builtin, lean_object* w) {
lean_object * res;
if (_G_initialized) return lean_io_result_mk_ok(lean_box(0));
_G_initialized = true;
res = initialize_Init(builtin, lean_io_mk_world());
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
res = initialize_Aura_Journal_Core(builtin, lean_io_mk_world());
if (lean_io_result_is_error(res)) return res;
lean_dec_ref(res);
l_Aura_Journal_instJoinSemilatticeJournal___closed__0 = _init_l_Aura_Journal_instJoinSemilatticeJournal___closed__0();
lean_mark_persistent(l_Aura_Journal_instJoinSemilatticeJournal___closed__0);
l_Aura_Journal_instJoinSemilatticeJournal = _init_l_Aura_Journal_instJoinSemilatticeJournal();
lean_mark_persistent(l_Aura_Journal_instJoinSemilatticeJournal);
return lean_io_result_mk_ok(lean_box(0));
}
#ifdef __cplusplus
}
#endif
