"use client"
import { useEffect, useState } from "react"
import { UserMenu } from "./UserMenu"
import type { Profile } from "../../global"
import { userLocale } from "../../i18n/locale"
import { useI18n } from "../../i18n/I18nContext"
export type LibraryFilter = "all" | "favorites" | "collections"
interface TopBarProps { profile?: Profile; activeTab:number; onTab:(i:number)=>void; onRefresh:()=>void; onOpenProfile:()=>void; menuOpen:boolean; onToggleMenu:()=>void; onCloseMenu:()=>void; showHidden:boolean; onToggleShowHidden:()=>void; libraryFilter?:LibraryFilter; onLibraryFilter?:(f:LibraryFilter)=>void; search?:string; onSearch?:(v:string)=>void }
const Icon=({children,className=""}:{children:React.ReactNode;className?:string})=><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" className={className} aria-hidden="true">{children}</svg>
export const TABS=["topbar.noticias","topbar.jogos"]
export function TopBar({profile,activeTab,onTab,onRefresh,onOpenProfile,menuOpen,onToggleMenu,onCloseMenu,showHidden,onToggleShowHidden,libraryFilter="all",onLibraryFilter,search="",onSearch}:TopBarProps){
 const {t}=useI18n(); const [now,setNow]=useState(new Date()); useEffect(()=>{const timer=setInterval(()=>setNow(new Date()),30000);return()=>clearInterval(timer)},[]); const initial=(profile?.name?.[0]||t("topbar.fallback_inicial")).toUpperCase()
 return <header className="ps5-topbar anim-nav fixed inset-x-0 top-0 z-30 flex h-[92px] items-center px-[4.5vw] text-white">
  <nav className="ps5-primary-nav flex items-center gap-9" aria-label="Navegação principal">{[{i:0,l:t("topbar.noticias")},{i:1,l:t("topbar.jogos")}].map(x=><button key={x.i} data-active={activeTab===x.i} onClick={()=>onTab(x.i)}>{x.l}</button>)}</nav>
  <div className="ml-auto flex items-center gap-5">{activeTab===1&&<label className="ps5-search"><Icon className="h-5 w-5"><circle cx="11" cy="11" r="7"/><path d="m20 20-4-4"/></Icon><input value={search} onChange={e=>onSearch?.(e.target.value)} placeholder="Buscar"/></label>}
   <time className="ps5-time">{now.toLocaleTimeString(userLocale(),{hour:"2-digit",minute:"2-digit"})}</time>
   <div className="relative"><button onClick={onToggleMenu} className="ps5-profile-button">{profile?.avatar?<img src={profile.avatar} alt=""/>:initial}</button><UserMenu open={menuOpen} onClose={onCloseMenu} onOpenProfile={onOpenProfile} onRefresh={onRefresh} showHidden={showHidden} onToggleShowHidden={onToggleShowHidden} profile={profile}/></div>
  </div>
 </header>
}
