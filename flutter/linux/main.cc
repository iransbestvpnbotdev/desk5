#include <dlfcn.h>
#include "my_application.h"

#define remotend_LIB_PATH "libremotend.so"
// #define remotend_LIB_PATH "/usr/lib/remotend/libremotend.so"
typedef bool (*remotendCoreMain)();
bool gIsConnectionManager = false;

bool flutter_remotend_core_main() {
   void* libremotend = dlopen(remotend_LIB_PATH, RTLD_LAZY);
   if (!libremotend) {
     fprintf(stderr,"load libremotend.so failed\n");
     return true;
   }
   auto core_main = (remotendCoreMain) dlsym(libremotend,"remotend_core_main");
   char* error;
   if ((error = dlerror()) != nullptr) {
       fprintf(stderr, "error finding remotend_core_main: %s", error);
       return true;
   }
   return core_main();
}

int main(int argc, char** argv) {
  if (!flutter_remotend_core_main()) {
      return 0;
  }
  for (int i = 0; i < argc; i++) {
    if (strcmp(argv[i], "--cm") == 0) {
      gIsConnectionManager = true;
    }
  }
  g_autoptr(MyApplication) app = my_application_new();
  return g_application_run(G_APPLICATION(app), argc, argv);
}
