# Diff
## /out.js
### esbuild
```js
// Users/user/project/node_modules/demo-pkg/index.js
var require_demo_pkg = __commonJS({
  "Users/user/project/node_modules/demo-pkg/index.js"(exports) {
    exports.foo = 123;
    console.log("hello");
  }
});

// Users/user/project/src/entry.js
var ns = __toESM(require_demo_pkg());
console.log(ns);
```
### rolldown
```js

```
### diff
```diff
===================================================================
--- esbuild	/out.js
+++ rolldown	
@@ -1,8 +0,0 @@
-var require_demo_pkg = __commonJS({
-    "Users/user/project/node_modules/demo-pkg/index.js"(exports) {
-        exports.foo = 123;
-        console.log("hello");
-    }
-});
-var ns = __toESM(require_demo_pkg());
-console.log(ns);

```