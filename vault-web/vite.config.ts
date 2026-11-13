import {defineConfig} from "vitest/config";
import react from "@vitejs/plugin-react";
export default defineConfig({plugins:[react()],worker:{format:"es"},server:{host:"127.0.0.1",port:5175,strictPort:true,proxy:Object.fromEntries(["/api","/session","/public"].map(p=>[p,{target:"http://127.0.0.1:3220",changeOrigin:false}]))},test:{environment:"jsdom",restoreMocks:true}});
