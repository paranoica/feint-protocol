#include <stdlib.h>
#include <string.h>

#include "feint_match.h"
#include "gdextension_interface.h"

#define EXPORT __attribute__((visibility("default")))
#define NOTIFY_HASH 4023243586

typedef struct {
  uint8_t data[8];
} StringName;

typedef struct {
  uint8_t data[8];
} String;


static struct {
  GDExtensionClassLibraryPtr library;
  GDExtensionInterfaceStringNameNewWithLatin1Chars string_name_new;
  GDExtensionInterfaceStringNewWithUtf8Chars string_new;
  GDExtensionInterfaceClassdbConstructObject3 construct;
  GDExtensionInterfaceObjectSetInstance set_instance;
  GDExtensionInterfaceClassdbGetMethodBind get_method_bind;
  GDExtensionInterfaceObjectMethodBindPtrcall bind_ptrcall;
  GDExtensionInterfaceClassdbRegisterExtensionClass6 register_class;
  GDExtensionInterfaceClassdbRegisterExtensionClassMethod register_method;
  GDExtensionInterfaceClassdbUnregisterExtensionClass unregister_class;
  GDExtensionInterfaceGetVariantFromTypeConstructor from_type;
  GDExtensionInterfaceGetVariantToTypeConstructor to_type;
  GDExtensionInterfaceVariantGetPtrDestructor destructor;
  GDExtensionInterfaceVariantGetType variant_type;
  GDExtensionMethodBindPtr notify;
  StringName class_name;
  StringName parent_name;
} api;

typedef enum { RET_BOOL, RET_INT, RET_FLOAT, RET_STRING } Ret;

typedef struct {
  const char *name;
  uint32_t argc;
  GDExtensionVariantType args[3];
  const char *arg_names[3];
  Ret ret;
} Method;

static const Method METHODS[] = {
    {"start", 3, {GDEXTENSION_VARIANT_TYPE_INT, GDEXTENSION_VARIANT_TYPE_INT, GDEXTENSION_VARIANT_TYPE_INT},
     {"seed", "level_a", "level_b"}, RET_BOOL},
    {"step", 2, {GDEXTENSION_VARIANT_TYPE_FLOAT, GDEXTENSION_VARIANT_TYPE_FLOAT}, {"x", "y"}, RET_INT},
    {"position", 1, {GDEXTENSION_VARIANT_TYPE_INT}, {"index"}, RET_FLOAT},
    {"layout", 1, {GDEXTENSION_VARIANT_TYPE_INT}, {"index"}, RET_FLOAT},
    {"score", 1, {GDEXTENSION_VARIANT_TYPE_INT}, {"seat"}, RET_INT},
    {"finished", 0, {0}, {0}, RET_BOOL},
    {"play_out", 1, {GDEXTENSION_VARIANT_TYPE_INT}, {"cap"}, RET_STRING},
};

#define METHOD_COUNT (sizeof METHODS / sizeof METHODS[0])

static int64_t as_int(GDExtensionConstVariantPtr v) {
  int64_t out = 0;
  api.to_type(GDEXTENSION_VARIANT_TYPE_INT)(&out, (GDExtensionVariantPtr)v);
  return out;
}

static double as_float(GDExtensionConstVariantPtr v) {
  double out = 0.0;
  api.to_type(GDEXTENSION_VARIANT_TYPE_FLOAT)(&out, (GDExtensionVariantPtr)v);
  return out;
}

typedef struct {
  int64_t i[3];
  double f[3];
} Args;

static void run(const Method *method, Match *m, const Args *a, uint8_t *b, int64_t *i, double *f, String *s) {
  char text[128];
  const char *name = method->name;
  if (strcmp(name, "start") == 0) {
    *b = (uint8_t)start(m, a->i[0], a->i[1], a->i[2]);
  } else if (strcmp(name, "step") == 0) {
    *i = step(m, a->f[0], a->f[1]);
  } else if (strcmp(name, "position") == 0) {
    *f = (a->i[0] >= 0 && a->i[0] < FEINT_POSITIONS && m->game != NULL) ? m->positions[a->i[0]] : 0.0;
  } else if (strcmp(name, "layout") == 0) {
    *f = (a->i[0] >= 0 && a->i[0] < FEINT_LAYOUT && m->game != NULL) ? m->layout[a->i[0]] : 0.0;
  } else if (strcmp(name, "score") == 0) {
    *i = score(m, a->i[0]);
  } else if (strcmp(name, "finished") == 0) {
    *b = (uint8_t)(m->game != NULL && feint_game_finished(m->game) == 1);
  } else if (strcmp(name, "play_out") == 0) {
    play_out(m, a->i[0], text, sizeof text);
    api.string_new(s, text);
  }
}

static void call(void *userdata, GDExtensionClassInstancePtr instance, const GDExtensionConstVariantPtr *args,
                 GDExtensionInt argc, GDExtensionVariantPtr ret, GDExtensionCallError *error) {
  const Method *method = userdata;
  if (argc < (GDExtensionInt)method->argc) {
    error->error = GDEXTENSION_CALL_ERROR_TOO_FEW_ARGUMENTS;
    error->expected = (int32_t)method->argc;
    return;
  }
  if (argc > (GDExtensionInt)method->argc) {
    error->error = GDEXTENSION_CALL_ERROR_TOO_MANY_ARGUMENTS;
    error->expected = (int32_t)method->argc;
    return;
  }
  Args a = {{0}, {0}};
  for (uint32_t k = 0; k < method->argc; k++) {
    GDExtensionVariantType given = api.variant_type(args[k]);
    int is_int = given == GDEXTENSION_VARIANT_TYPE_INT;
    int is_float = given == GDEXTENSION_VARIANT_TYPE_FLOAT;
    if (method->args[k] == GDEXTENSION_VARIANT_TYPE_INT && is_int) {
      a.i[k] = as_int(args[k]);
    } else if (method->args[k] == GDEXTENSION_VARIANT_TYPE_FLOAT && (is_float || is_int)) {
      a.f[k] = is_int ? (double)as_int(args[k]) : as_float(args[k]);
    } else {
      error->error = GDEXTENSION_CALL_ERROR_INVALID_ARGUMENT;
      error->argument = (int32_t)k;
      error->expected = (int32_t)method->args[k];
      return;
    }
  }
  uint8_t b = 0;
  int64_t i = 0;
  double f = 0.0;
  String s;
  memset(&s, 0, sizeof s);
  run(method, instance, &a, &b, &i, &f, &s);
  error->error = GDEXTENSION_CALL_OK;
  switch (method->ret) {
    case RET_BOOL:
      api.from_type(GDEXTENSION_VARIANT_TYPE_BOOL)(ret, &b);
      break;
    case RET_INT:
      api.from_type(GDEXTENSION_VARIANT_TYPE_INT)(ret, &i);
      break;
    case RET_FLOAT:
      api.from_type(GDEXTENSION_VARIANT_TYPE_FLOAT)(ret, &f);
      break;
    case RET_STRING:
      api.from_type(GDEXTENSION_VARIANT_TYPE_STRING)(ret, &s);
      api.destructor(GDEXTENSION_VARIANT_TYPE_STRING)(&s);
      break;
  }
}

static void ptrcall(void *userdata, GDExtensionClassInstancePtr instance, const GDExtensionConstTypePtr *args,
                    GDExtensionTypePtr ret) {
  const Method *method = userdata;
  Args a = {{0}, {0}};
  for (uint32_t k = 0; k < method->argc; k++) {
    if (method->args[k] == GDEXTENSION_VARIANT_TYPE_INT) {
      a.i[k] = *(const int64_t *)args[k];
    } else {
      a.f[k] = *(const double *)args[k];
    }
  }
  uint8_t b = 0;
  int64_t i = 0;
  double f = 0.0;
  if (method->ret == RET_STRING) {
    api.destructor(GDEXTENSION_VARIANT_TYPE_STRING)(ret);
  }
  run(method, instance, &a, &b, &i, &f, (String *)ret);
  switch (method->ret) {
    case RET_BOOL:
      *(uint8_t *)ret = b;
      break;
    case RET_INT:
      *(int64_t *)ret = i;
      break;
    case RET_FLOAT:
      *(double *)ret = f;
      break;
    case RET_STRING:
      break;
  }
}

static GDExtensionObjectPtr create(void *userdata, GDExtensionBool notify) {
  (void)userdata;
  Match *m = calloc(1, sizeof(Match));
  if (m == NULL) {
    return NULL;
  }
  GDExtensionObjectPtr object = api.construct(&api.parent_name);
  if (object == NULL) {
    free(m);
    return NULL;
  }
  api.set_instance(object, &api.class_name, m);
  if (notify) {
    int64_t what = 0;
    uint8_t reversed = 0;
    GDExtensionConstTypePtr args[2] = {&what, &reversed};
    api.bind_ptrcall(api.notify, object, args, NULL);
  }
  return object;
}

static void destroy(void *userdata, GDExtensionClassInstancePtr instance) {
  (void)userdata;
  Match *m = instance;
  if (m != NULL) {
    close_match(m);
    free(m);
  }
}

static GDExtensionVariantType ret_type(Ret r) {
  switch (r) {
    case RET_BOOL:
      return GDEXTENSION_VARIANT_TYPE_BOOL;
    case RET_INT:
      return GDEXTENSION_VARIANT_TYPE_INT;
    case RET_FLOAT:
      return GDEXTENSION_VARIANT_TYPE_FLOAT;
    case RET_STRING:
      return GDEXTENSION_VARIANT_TYPE_STRING;
  }
  return GDEXTENSION_VARIANT_TYPE_NIL;
}

static GDExtensionClassMethodArgumentMetadata metadata(GDExtensionVariantType t) {
  switch (t) {
    case GDEXTENSION_VARIANT_TYPE_INT:
      return GDEXTENSION_METHOD_ARGUMENT_METADATA_INT_IS_INT64;
    case GDEXTENSION_VARIANT_TYPE_FLOAT:
      return GDEXTENSION_METHOD_ARGUMENT_METADATA_REAL_IS_DOUBLE;
    default:
      return GDEXTENSION_METHOD_ARGUMENT_METADATA_NONE;
  }
}

static void register_methods(void) {
  static StringName names[METHOD_COUNT];
  static StringName arg_names[METHOD_COUNT][3];
  static StringName empty;
  static String hint;
  api.string_name_new(&empty, "", 1);
  api.string_new(&hint, "");
  for (size_t k = 0; k < METHOD_COUNT; k++) {
    const Method *method = &METHODS[k];
    api.string_name_new(&names[k], method->name, 1);
    GDExtensionPropertyInfo args[3];
    GDExtensionClassMethodArgumentMetadata meta[3];
    for (uint32_t a = 0; a < method->argc; a++) {
      api.string_name_new(&arg_names[k][a], method->arg_names[a], 1);
      args[a] = (GDExtensionPropertyInfo){method->args[a], &arg_names[k][a], &empty, 0, &hint, 6};
      meta[a] = metadata(method->args[a]);
    }
    GDExtensionVariantType rt = ret_type(method->ret);
    GDExtensionPropertyInfo ret = {rt, &empty, &empty, 0, &hint, 6};
    GDExtensionClassMethodInfo info = {
        .name = &names[k],
        .method_userdata = (void *)method,
        .call_func = call,
        .ptrcall_func = ptrcall,
        .method_flags = GDEXTENSION_METHOD_FLAGS_DEFAULT,
        .has_return_value = 1,
        .return_value_info = &ret,
        .return_value_metadata = metadata(rt),
        .argument_count = method->argc,
        .arguments_info = args,
        .arguments_metadata = meta,
        .default_argument_count = 0,
        .default_arguments = NULL,
    };
    api.register_method(api.library, &api.class_name, &info);
  }
}

static void initialize(void *userdata, GDExtensionInitializationLevel level) {
  (void)userdata;
  if (level != GDEXTENSION_INITIALIZATION_SCENE) {
    return;
  }
  api.string_name_new(&api.class_name, "FeintMatch", 1);
  api.string_name_new(&api.parent_name, "RefCounted", 1);
  StringName object_name;
  StringName notification_name;
  api.string_name_new(&object_name, "Object", 1);
  api.string_name_new(&notification_name, "notification", 1);
  api.notify = api.get_method_bind(&object_name, &notification_name, NOTIFY_HASH);
  if (api.notify == NULL) {
    return;
  }
  GDExtensionClassCreationInfo6 info;
  memset(&info, 0, sizeof info);
  info.is_exposed = 1;
  info.create_instance_func = create;
  info.free_instance_func = destroy;
  api.register_class(api.library, &api.class_name, &api.parent_name, &info);
  register_methods();
}

static void deinitialize(void *userdata, GDExtensionInitializationLevel level) {
  (void)userdata;
  if (level != GDEXTENSION_INITIALIZATION_SCENE) {
    return;
  }
  api.unregister_class(api.library, &api.class_name);
}

EXPORT GDExtensionBool feint_godot_init(GDExtensionInterfaceGetProcAddress get, GDExtensionClassLibraryPtr library,
                                        GDExtensionInitialization *init) {
  api.library = library;
  api.string_name_new = (GDExtensionInterfaceStringNameNewWithLatin1Chars)get("string_name_new_with_latin1_chars");
  api.string_new = (GDExtensionInterfaceStringNewWithUtf8Chars)get("string_new_with_utf8_chars");
  api.construct = (GDExtensionInterfaceClassdbConstructObject3)get("classdb_construct_object3");
  api.set_instance = (GDExtensionInterfaceObjectSetInstance)get("object_set_instance");
  api.get_method_bind = (GDExtensionInterfaceClassdbGetMethodBind)get("classdb_get_method_bind");
  api.bind_ptrcall = (GDExtensionInterfaceObjectMethodBindPtrcall)get("object_method_bind_ptrcall");
  api.register_class = (GDExtensionInterfaceClassdbRegisterExtensionClass6)get("classdb_register_extension_class6");
  api.register_method =
      (GDExtensionInterfaceClassdbRegisterExtensionClassMethod)get("classdb_register_extension_class_method");
  api.unregister_class =
      (GDExtensionInterfaceClassdbUnregisterExtensionClass)get("classdb_unregister_extension_class");
  api.from_type = (GDExtensionInterfaceGetVariantFromTypeConstructor)get("get_variant_from_type_constructor");
  api.to_type = (GDExtensionInterfaceGetVariantToTypeConstructor)get("get_variant_to_type_constructor");
  api.destructor = (GDExtensionInterfaceVariantGetPtrDestructor)get("variant_get_ptr_destructor");
  api.variant_type = (GDExtensionInterfaceVariantGetType)get("variant_get_type");
  if (api.string_name_new == NULL || api.string_new == NULL || api.construct == NULL || api.set_instance == NULL ||
      api.get_method_bind == NULL || api.bind_ptrcall == NULL || api.register_class == NULL ||
      api.register_method == NULL || api.unregister_class == NULL || api.variant_type == NULL || api.from_type == NULL || api.to_type == NULL || api.destructor == NULL) {
    return 0;
  }
  init->minimum_initialization_level = GDEXTENSION_INITIALIZATION_SCENE;
  init->userdata = NULL;
  init->initialize = initialize;
  init->deinitialize = deinitialize;
  return 1;
}
