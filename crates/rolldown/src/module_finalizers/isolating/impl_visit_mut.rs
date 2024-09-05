use oxc::ast::ast::{self, ExportDefaultDeclarationKind, Expression, Statement};
use oxc::ast::visit::walk_mut;
use oxc::ast::VisitMut;
use oxc::span::{CompactStr, Span, SPAN};
use rolldown_common::{Interop, Module};
use rolldown_ecmascript::TakeIn;
use rolldown_utils::ecma_script::legitimize_identifier_name;

use super::IsolatingModuleFinalizer;

impl<'me, 'ast> VisitMut<'ast> for IsolatingModuleFinalizer<'me, 'ast> {
  fn visit_program(&mut self, program: &mut ast::Program<'ast>) {
    let mut stmts = self.snippet.builder.vec();

    for mut stmt in program.body.take_in(self.alloc) {
      walk_mut::walk_statement(self, &mut stmt);
      match &mut stmt {
        Statement::ImportDeclaration(import_decl) => {
          self.transform_import_declaration(import_decl);
        }
        ast::Statement::ExportDefaultDeclaration(export_default_decl) => {
          stmts.push(self.transform_export_default_declaration(export_default_decl));
        }
        ast::Statement::ExportNamedDeclaration(export_named_decl) => {
          self.transform_named_declaration(export_named_decl);
        }
        ast::Statement::ExportAllDeclaration(export_all_decl) => {
          self.transform_export_all_declaration(export_all_decl);
        }
        _ => stmts.push(stmt),
      };
    }

    // Add __esModule flag for esm module
    if self.ctx.module.exports_kind.is_esm() {
      program.body.push(self.snippet.builder.statement_expression(
        SPAN,
        self.snippet.call_expr_with_arg_expr("__toCommonJS", "exports"),
      ));
    }

    // Generate export statements, using `Object.defineProperty`
    if !self.generated_exports.is_empty() {
      program.body.push(self.snippet.builder.statement_expression(
        SPAN,
        self.snippet.alloc_call_expr_with_2arg_expr_expr(
          "__export",
          self.snippet.id_ref_expr("exports", SPAN),
          Expression::ObjectExpression(self.snippet.builder.alloc_object_expression(
            SPAN,
            self.snippet.builder.vec_from_iter(self.generated_exports.drain(..)),
            None,
          )),
        ),
      ));
    }

    // Add generated imports
    program.body.extend(self.generated_imports.drain(..));

    program.body.extend(stmts);
  }

  fn visit_expression(&mut self, expr: &mut Expression<'ast>) {
    if let Expression::Identifier(ident) = expr {
      if let Some(named_import) = ident
        .reference_id
        .get()
        .and_then(|reference_id| self.scope.symbol_id_for(reference_id))
        .map(|symbol_id| (self.ctx.module.idx, symbol_id).into())
        .and_then(|symbol_ref| self.ctx.module.named_imports.get(&symbol_ref))
      {
        let rec = &self.ctx.module.import_records[named_import.record_id];
        match &self.ctx.modules[rec.resolved_module] {
          Module::Ecma(importee) => {
            // TODO deconflict namespace_ref
            let namespace_ref = self.ctx.symbols.get_original_name(importee.namespace_object_ref);

            match &named_import.imported {
              rolldown_common::Specifier::Star => {
                ident.name = self.snippet.atom(namespace_ref.as_str());
              }
              rolldown_common::Specifier::Literal(imported) => {
                *expr = Expression::StaticMemberExpression(
                  self.snippet.builder.alloc_static_member_expression(
                    ident.span,
                    self.snippet.id_ref_expr(namespace_ref, SPAN),
                    self.snippet.builder.identifier_name(SPAN, imported.as_str()),
                    false,
                  ),
                );
              }
            }
          }
          Module::External(_) => {}
        }
      };
    }
    walk_mut::walk_expression(self, expr);
  }

  fn visit_static_member_expression(&mut self, expr: &mut ast::StaticMemberExpression<'ast>) {
    // replace `import.meta.hot` -> `module.hot`
    if let Expression::MetaProperty(meta) = &expr.object {
      if expr.property.name == "hot" && meta.meta.name == "import" && meta.property.name == "meta" {
        expr.object = self.snippet.id_ref_expr("module", SPAN);
      }
    }

    walk_mut::walk_static_member_expression(self, expr);
  }
}

impl<'me, 'ast> IsolatingModuleFinalizer<'me, 'ast> {
  pub fn transform_import_declaration(&mut self, import_decl: &ast::ImportDeclaration<'ast>) {
    // The specifiers rewrite with reference the namespace object, see `IsolatingModuleFinalizer#visit_expression`

    // Create a require call statement for import declaration
    let module = self.get_importee_module(import_decl.span);
    let namespace_object_ref = self.create_namespace_object_ref_for_module(module);
    self.create_require_call_stmt(
      &module.stable_id().into(),
      module.interop(),
      &namespace_object_ref,
      import_decl.span,
    );
  }

  pub fn transform_export_default_declaration(
    &mut self,
    export_default_decl: &mut ast::ExportDefaultDeclaration<'ast>,
  ) -> Statement<'ast> {
    // TODO deconflict default_export_ref
    let default_export_ref = self.ctx.symbols.get_original_name(self.ctx.module.default_export_ref);

    match &mut export_default_decl.declaration {
      decl @ ast::match_expression!(ExportDefaultDeclarationKind) => {
        self.generated_exports.push(self.snippet.object_property_kind_object_property(
          "default",
          self.snippet.id_ref_expr(default_export_ref, SPAN),
          false,
        ));
        self
          .snippet
          .builder
          .statement_expression(SPAN, decl.to_expression_mut().take_in(self.alloc))
      }
      ast::ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
        let from =
          func.id.as_ref().map_or(default_export_ref.as_str(), |ident| ident.name.as_str());
        self.generated_exports.push(self.snippet.object_property_kind_object_property(
          "default",
          self.snippet.id_ref_expr(from, SPAN),
          false,
        ));
        self
          .snippet
          .builder
          .statement_expression(SPAN, Expression::FunctionExpression(func.take_in(self.alloc)))
      }
      ast::ExportDefaultDeclarationKind::ClassDeclaration(class) => {
        let from =
          class.id.as_ref().map_or(default_export_ref.as_str(), |ident| ident.name.as_str());
        self.generated_exports.push(self.snippet.object_property_kind_object_property(
          "default",
          self.snippet.id_ref_expr(from, SPAN),
          false,
        ));
        self
          .snippet
          .builder
          .statement_expression(SPAN, Expression::ClassExpression(class.take_in(self.alloc)))
      }
      ast::ExportDefaultDeclarationKind::TSInterfaceDeclaration(_) => {
        unreachable!("ExportDefaultDeclaration TSInterfaceDeclaration should be removed")
      }
    }
  }

  pub fn transform_named_declaration(
    &mut self,
    export_named_decl: &mut ast::ExportNamedDeclaration<'ast>,
  ) {
    match &export_named_decl.source {
      Some(_) => {
        let module = self.get_importee_module(export_named_decl.span);
        let namespace_object_ref = self.create_namespace_object_ref_for_module(module);
        self.create_require_call_stmt(
          &module.stable_id().into(),
          module.interop(),
          &namespace_object_ref,
          export_named_decl.span,
        );

        self.generated_exports.extend(export_named_decl.specifiers.iter().map(|specifier| {
          self.snippet.object_property_kind_object_property(
            &specifier.exported.name(),
            match &specifier.local {
              ast::ModuleExportName::IdentifierName(ident) => {
                Expression::StaticMemberExpression(
                  self.snippet.builder.alloc_static_member_expression(
                    SPAN,
                    self.snippet.id_ref_expr(&namespace_object_ref, SPAN),
                    self.snippet.builder.identifier_name(SPAN, ident.name.as_str()),
                    false,
                  ),
                )
              }
              ast::ModuleExportName::StringLiteral(str) => {
                Expression::ComputedMemberExpression(
                  self.snippet.builder.alloc_computed_member_expression(
                    SPAN,
                    self.snippet.id_ref_expr(&namespace_object_ref, SPAN),
                    self.snippet.builder.expression_from_string_literal(
                      self.snippet.builder.string_literal(SPAN, str.value.as_str()),
                    ),
                    false,
                  ),
                )
              }
              ast::ModuleExportName::IdentifierReference(_) => {
                unreachable!(
                  "ModuleExportName IdentifierReference is invalid in ExportNamedDeclaration with source"
                )
              }
            },
            matches!(specifier.exported, ast::ModuleExportName::StringLiteral(_))
          )
        }));
      }
      None => {
        self.generated_exports.extend(export_named_decl.specifiers.iter().map(|specifier| {
          self.snippet.object_property_kind_object_property(
            &specifier.exported.name(),
            match &specifier.local {
              ast::ModuleExportName::IdentifierName(ident) => {
                self.snippet.id_ref_expr(ident.name.as_str(), SPAN)
              }
              ast::ModuleExportName::StringLiteral(_) => {
                unreachable!("ModuleExportName StringLiteral is invalid in ExportNamedDeclaration without source")
              }
              ast::ModuleExportName::IdentifierReference(ident) => {
                self.snippet.id_ref_expr(ident.name.as_str(), SPAN)
              }
            },
            matches!(specifier.exported, ast::ModuleExportName::StringLiteral(_)
          ))
        }));
      }
    }
  }

  pub fn transform_export_all_declaration(
    &mut self,
    export_all_decl: &ast::ExportAllDeclaration<'ast>,
  ) {
    let module = self.get_importee_module(export_all_decl.span);
    let namespace_object_ref = self.create_namespace_object_ref_for_module(module);
    self.create_require_call_stmt(
      &module.stable_id().into(),
      module.interop(),
      &namespace_object_ref,
      export_all_decl.span,
    );

    match &export_all_decl.exported {
      Some(exported) => {
        self.generated_exports.push(self.snippet.object_property_kind_object_property(
          &exported.name(),
          self.snippet.id_ref_expr(&namespace_object_ref, SPAN),
          matches!(exported, ast::ModuleExportName::StringLiteral(_)),
        ));
      }
      None => {
        self.generated_imports.push(self.snippet.builder.statement_expression(
          SPAN,
          self.snippet.call_expr_with_2arg_expr("__reExport", "exports", &namespace_object_ref),
        ));
      }
    }
  }

  fn create_require_call_stmt(
    &mut self,
    module_stable_id: &CompactStr,
    interop: Option<Interop>,
    namespace_object_ref: &CompactStr,
    span: Span,
  ) {
    if self.generated_imports_set.contains(namespace_object_ref) {
      return;
    }

    self.generated_imports_set.insert(namespace_object_ref.clone());

    let require_call = self.snippet.require_call_expr(module_stable_id.as_str());

    self.generated_imports.push(self.snippet.variable_declarator_require_call_stmt(
      namespace_object_ref,
      self.snippet.to_esm_call_with_interop("__toESM", require_call, interop),
      span,
    ));
  }

  fn create_namespace_object_ref_for_module(&self, module: &Module) -> CompactStr {
    match module {
      Module::Ecma(importee) => {
        // TODO deconflict namespace_ref
        self.ctx.symbols.get_original_name(importee.namespace_object_ref).clone()
      }
      Module::External(external_module) => {
        // TODO need to generate one symbol and deconflict it
        legitimize_identifier_name(&external_module.name).to_string().into()
      }
    }
  }

  fn get_importee_module(&self, span: Span) -> &Module {
    let rec_id = self.ctx.module.imports[&span];
    let rec = &self.ctx.module.import_records[rec_id];
    &self.ctx.modules[rec.resolved_module]
  }
  fn visit_call_expression(&mut self, expr: &mut ast::CallExpression<'ast>) {
    if expr.is_global_require_call(self.scope) {
      if let Some(ast::Argument::StringLiteral(request)) = expr.arguments.first_mut() {
        let rec_id = self.ctx.module.imports[&expr.span];
        let resolved_module = self.ctx.module.import_records[rec_id].resolved_module;
        request.value = self.snippet.atom(self.ctx.modules[resolved_module].stable_id());
      }
    }

    walk_mut::walk_call_expression(self, expr);
  }

  fn visit_static_member_expression(&mut self, expr: &mut ast::StaticMemberExpression<'ast>) {
    // replace `import.meta.hot` -> `module.hot`
    if let Expression::MetaProperty(meta) = &expr.object {
      if expr.property.name == "hot" && meta.meta.name == "import" && meta.property.name == "meta" {
        expr.object = self.snippet.id_ref_expr("module", SPAN);
      }
    }

    walk_mut::walk_static_member_expression(self, expr);
  }

  // fn visit_program(&mut self, program: &mut ast::Program<'ast>) {
  //   let original_body = program.body.take_in(self.alloc);

  //   for stmt in original_body {
  //     match &stmt {
  //       // // rewrite:
  //       // - `import { default, a, b as b2 } from 'xxx'` to `const { default, a, b: b2 } = __static_import('xxx')`
  //       // - `import foo from 'xxx'` to `const { default: foo } = __static_import('xxx')`
  //       // - `import * as star from 'xxx'` to `const star = __static_import_star('xxx')`
  //       Statement::ImportDeclaration(import_decl) => {
  //         let rec_id = self.ctx.module.imports[&import_decl.span];
  //         let rec = &self.ctx.module.import_records[rec_id];
  //         let mut named_specifiers = vec![];
  //         let mut star_specifier = None;
  //         match &self.ctx.modules[rec.resolved_module] {
  //           Module::Ecma(importee) => {
  //             if let Some(specifiers) = &import_decl.specifiers {
  //               for specifier in specifiers {
  //                 match specifier {
  //                   ast::ImportDeclarationSpecifier::ImportSpecifier(s) => {
  //                     named_specifiers.push((s.imported.name().as_str(), s.local.name.as_str()));
  //                   }
  //                   ast::ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
  //                     named_specifiers.push(("default", s.local.name.as_str()));
  //                   }
  //                   ast::ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
  //                     star_specifier = Some(s);
  //                   }
  //                 }
  //               }
  //             }
  //             let is_plain_import =
  //               import_decl.specifiers.as_ref().map_or(false, |specifiers| specifiers.is_empty());
  //             let importee = &self.ctx.modules[importee.idx];
  //             if is_plain_import {
  //               program.body.push(
  //                 self
  //                   .snippet
  //                   .app_static_import_call_multiple_specifiers_stmt(&[], importee.stable_id()),
  //               );
  //               continue;
  //             } else if let Some(star_spec) = star_specifier {
  //               program.body.push(
  //                 self
  //                   .snippet
  //                   .app_static_import_star_call_stmt(&star_spec.local.name, importee.stable_id()),
  //               );
  //               continue;
  //             }
  //             program.body.push(self.snippet.app_static_import_call_multiple_specifiers_stmt(
  //               &named_specifiers,
  //               importee.stable_id(),
  //             ));
  //             continue;
  //           }
  //           Module::External(_) => unimplemented!(),
  //         }
  //       }
  //       // TODO: rewrite `export default xxx` to `var __rolldown_default_export__ = xxx`
  //       ast::Statement::ExportDefaultDeclaration(_default_decl) => {}
  //       _ => {}
  //     }
  //     program.body.push(stmt);
  //   }
  // }
}
